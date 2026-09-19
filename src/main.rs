//! The `memtree` command line.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use memtree::affected::{self, Reason};
use memtree::check::check;
use memtree::finding::Severity;
use memtree::index::{self, Update};
use memtree::links;
use memtree::store::{self, INDEX_FILE};

/// Exit status when the store has errors.
const FINDINGS: u8 = 1;
/// Exit status when the command line is wrong.
const USAGE: u8 = 2;
/// Exit status when memtree could not do its job.
const FAILURE: u8 = 3;

const USAGE_TEXT: &str = "\
usage: memtree check [--root DIR]
       memtree index [--root DIR]
       memtree affected --base REF [--root DIR]
       memtree links [--root DIR] [NAME...]
       memtree help | --version";

/// What `memtree help` prints before the usage.
const ABOUT_TEXT: &str = "\
memtree checks, indexes, and diffs a markdown memory store: notes with
frontmatter, [[name]] links, and a MEMORY.md index.";

/// What `memtree help` prints after the usage.
const DETAILS_TEXT: &str = "\
commands:
  check     report errors and warnings in the store; writes nothing
  index     append an entry to MEMORY.md for each note it does not list,
            keeping every existing line
  affected  list the notes added, changed, or deleted since the merge base,
            and the notes that link to them; writes nothing
  links     name every note, or list the links that target a given name;
            writes nothing

options:
  --root DIR     the store root (default: the current directory)
  --base REF     changes are measured from the merge base of REF and HEAD
  -h, --help     print this help
  -V, --version  print the version

exit status: 0 success, 1 the store has errors, 2 usage error, 3 failure";

enum Command {
    Help,
    Version,
    Check { root: PathBuf },
    Index { root: PathBuf },
    Affected { root: PathBuf, base: String },
    Links { root: PathBuf, names: Vec<String> },
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let status = match parse(&args) {
        Ok(command) => run(&command),
        Err(message) => {
            report(format_args!("{message}\n{USAGE_TEXT}"));
            USAGE
        }
    };
    ExitCode::from(status)
}

fn parse(args: &[OsString]) -> Result<Command, String> {
    let args = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .ok_or_else(|| format!("argument `{}` is not valid UTF-8", arg.to_string_lossy()))
        })
        .collect::<Result<Vec<&str>, String>>()?;
    let Some((command, rest)) = args.split_first() else {
        return Err("missing command".to_owned());
    };
    match *command {
        "help" | "-h" | "--help" => no_arguments(rest).map(|()| Command::Help),
        "-V" | "--version" => no_arguments(rest).map(|()| Command::Version),
        "check" => Ok(Command::Check {
            root: root(&options(rest, &["--root"])?),
        }),
        "index" => Ok(Command::Index {
            root: root(&options(rest, &["--root"])?),
        }),
        "affected" => {
            let options = options(rest, &["--base", "--root"])?;
            let Some(base) = options.get("--base").copied() else {
                return Err("`affected` needs `--base REF`".to_owned());
            };
            if base.starts_with('-') {
                return Err(format!("a ref cannot start with `-`: `{base}`"));
            }
            Ok(Command::Affected {
                root: root(&options),
                base: base.to_owned(),
            })
        }
        "links" => {
            let (root, names) = links_arguments(rest)?;
            Ok(Command::Links { root, names })
        }
        other if other.starts_with('-') => {
            Err(format!("unknown option `{}`", split_inline(other).0))
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

/// Parses `links`'s arguments: `--root DIR` (at most once) plus NAME positionals that never
/// start with `-`, in the order they were given.
fn links_arguments(args: &[&str]) -> Result<(PathBuf, Vec<String>), String> {
    let mut root: Option<&str> = None;
    let mut names = Vec::new();
    let mut rest = args.iter().copied();
    while let Some(arg) = rest.next() {
        let (flag, inline) = split_inline(arg);
        if flag == "--root" && arg.starts_with("--") {
            let value = match inline {
                Some(value) => value,
                None => rest.next().unwrap_or_default(),
            };
            if value.is_empty() {
                return Err("`--root` needs a value".to_owned());
            }
            if root.replace(value).is_some() {
                return Err("`--root` is given more than once".to_owned());
            }
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown option `{flag}`"));
        }
        names.push(arg.to_owned());
    }
    Ok((PathBuf::from(root.unwrap_or(".")), names))
}

/// `help` and `--version` take no arguments.
fn no_arguments(rest: &[&str]) -> Result<(), String> {
    match rest.first() {
        Some(arg) => Err(format!("unexpected argument `{arg}`")),
        None => Ok(()),
    }
}

/// Splits `--name=VALUE` into its name and value. Any other argument has no inline value.
fn split_inline(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
        _ => (arg, None),
    }
}

/// The store root from `--root DIR`, which defaults to the current directory.
fn root(options: &BTreeMap<&str, &str>) -> PathBuf {
    PathBuf::from(options.get("--root").copied().unwrap_or("."))
}

/// Parses `--name VALUE` and `--name=VALUE` options from `allowed`, each at most once.
fn options<'a>(
    args: &[&'a str],
    allowed: &[&'static str],
) -> Result<BTreeMap<&'static str, &'a str>, String> {
    let mut values = BTreeMap::new();
    let mut rest = args.iter().copied();
    while let Some(arg) = rest.next() {
        let (flag, inline) = split_inline(arg);
        let Some(name) = allowed.iter().copied().find(|name| *name == flag) else {
            return Err(if arg.starts_with('-') {
                format!("unknown option `{flag}`")
            } else {
                format!("unexpected argument `{arg}`")
            });
        };
        let value = match inline {
            Some(value) => value,
            None => rest.next().unwrap_or_default(),
        };
        if value.is_empty() {
            return Err(format!("`{name}` needs a value"));
        }
        if values.insert(name, value).is_some() {
            return Err(format!("`{name}` is given more than once"));
        }
    }
    Ok(values)
}

fn run(command: &Command) -> u8 {
    match command {
        Command::Help => run_print(&format!("{ABOUT_TEXT}\n\n{USAGE_TEXT}\n\n{DETAILS_TEXT}")),
        Command::Version => run_print(&format!("memtree {}", env!("CARGO_PKG_VERSION"))),
        Command::Check { root } => run_check(root),
        Command::Index { root } => run_index(root),
        Command::Affected { root, base } => run_affected(root, base),
        Command::Links { root, names } => run_links(root, names),
    }
}

/// Prints the text for `help` or `--version`.
fn run_print(text: &str) -> u8 {
    match print_lines(&[text]) {
        Ok(()) => 0,
        Err(status) => status,
    }
}

/// Lists every `name path` pair, or every link line that targets one of `names`.
fn run_links(root: &Path, names: &[String]) -> u8 {
    let store = match store::load(root) {
        Ok(store) => store,
        Err(error) => return fail(format_args!("{error}")),
    };
    let wanted: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let mut lines: Vec<String> = Vec::new();
    if wanted.is_empty() {
        for note in &store.notes {
            if let Some((name, _)) = &note.name {
                lines.push(format!("{name} {}", note.path));
            }
        }
    } else {
        for note in &store.notes {
            for link in links::dedup_by_position(&note.links) {
                if wanted.contains(link.target.as_str()) {
                    lines.push(format!(
                        "{}:{}: links to [[{}]]",
                        note.path, link.line, link.target
                    ));
                }
            }
        }
    }
    lines.sort();
    if let Err(status) = print_lines(&lines) {
        return status;
    }
    let links_counted = if wanted.is_empty() {
        store.notes.iter().map(|note| note.links.len()).sum()
    } else {
        lines.len()
    };
    report(format_args!(
        "links over {}: {}",
        count(store.notes.len(), "note"),
        count(links_counted, "link")
    ));
    0
}

fn run_check(root: &Path) -> u8 {
    let store = match store::load(root) {
        Ok(store) => store,
        Err(error) => return fail(error),
    };
    let findings = check(&store);
    if let Err(status) = print_lines(&findings) {
        return status;
    }
    let errors = findings
        .iter()
        .filter(|finding| finding.severity() == Severity::Error)
        .count();
    report(format_args!(
        "checked {}: {}, {}",
        count(store.notes.len(), "note"),
        count(errors, "error"),
        count(findings.len() - errors, "warning")
    ));
    if errors == 0 { 0 } else { FINDINGS }
}

fn run_index(root: &Path) -> u8 {
    let store = match store::load(root) {
        Ok(store) => store,
        Err(error) => return fail(error),
    };
    match index::update(&store) {
        Update::UpToDate => {
            report(format_args!("{INDEX_FILE} is up to date"));
            0
        }
        Update::Blocked(findings) => {
            if let Err(status) = print_lines(&findings) {
                return status;
            }
            report(format_args!(
                "{INDEX_FILE} not changed: {}",
                count(findings.len(), "error")
            ));
            FINDINGS
        }
        Update::Write {
            text,
            appended,
            created,
        } => {
            if let Err(error) = index::write(root, &text) {
                return fail(error);
            }
            if let Err(status) = print_lines(&appended) {
                return status;
            }
            let notes = count(appended.len(), "note");
            if created {
                report(format_args!("created {INDEX_FILE} with {notes}"));
            } else {
                report(format_args!("indexed {notes} in {INDEX_FILE}"));
            }
            0
        }
    }
}

fn run_affected(root: &Path, base: &str) -> u8 {
    let store = match store::load(root) {
        Ok(store) => store,
        Err(error) => return fail(error),
    };
    let affected = match affected::affected(root, &store, base) {
        Ok(affected) => affected,
        Err(error) => return fail(error),
    };
    if let Err(status) = print_lines(&affected) {
        return status;
    }
    let is_link = |reason: &Reason| matches!(reason, Reason::LinksTo(_));
    let touched = affected
        .iter()
        .filter(|entry| !is_link(&entry.reason))
        .count();
    let linking: BTreeSet<&str> = affected
        .iter()
        .filter(|entry| is_link(&entry.reason))
        .map(|entry| entry.path.as_str())
        .collect();
    report(format_args!(
        "since the merge base with `{base}`: {}, {}",
        count(touched, "touched note"),
        count(linking.len(), "linking note")
    ));
    0
}

/// Writes each item on its own line to stdout, or returns the failure status.
fn print_lines<T: Display>(items: &[T]) -> Result<(), u8> {
    let mut out = String::new();
    for item in items {
        out.push_str(&item.to_string());
        out.push('\n');
    }
    io::stdout()
        .lock()
        .write_all(out.as_bytes())
        .map_err(|error| fail(format_args!("cannot write output: {error}")))
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Writes `memtree: message` to stderr. A failure to write there has nowhere to be reported.
fn report(message: impl Display) {
    let _ = writeln!(io::stderr(), "memtree: {message}");
}

fn fail(message: impl Display) -> u8 {
    report(message);
    FAILURE
}
