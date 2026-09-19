//! `[[name]]` links in a note's body.

use std::collections::BTreeSet;

/// A link to another note, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The linked note's name, trimmed.
    pub target: String,
    /// 1-based line of the link.
    pub line: usize,
}

/// The links of one note, once per position (path, line, name).
///
/// A repeated `[[name]]` on the same line is one occurrence for reporting; the count of raw
/// links is still every found link.
#[must_use]
pub fn dedup_by_position(links: &[Link]) -> Vec<&Link> {
    let mut unique = BTreeSet::new();
    links
        .iter()
        .filter(|link| unique.insert((&link.target, link.line)))
        .collect()
}

/// Extracts the links in `text`, ignoring its first `skip` lines (the frontmatter block).
///
/// A link is `[[`, then text containing no `[` or `]`, then `]]`. Links are found left to right
/// without overlapping; a link whose trimmed target is empty is dropped. Fenced code blocks, and
/// inline code spans within a line, hold no links.
#[must_use]
pub fn extract(text: &str, skip: usize) -> Vec<Link> {
    let mut links = Vec::new();
    let mut fence: Option<Fence> = None;
    for (index, line) in text.lines().enumerate().skip(skip) {
        if let Some(open) = fence {
            if open.is_closed_by(line) {
                fence = None;
            }
            continue;
        }
        fence = Fence::opened_by(line);
        if fence.is_none() {
            for segment in outside_code_spans(line) {
                links_in(segment, index + 1, &mut links);
            }
        }
    }
    links
}

/// An open fenced code block.
#[derive(Debug, Clone, Copy)]
struct Fence {
    marker: u8,
    len: usize,
}

impl Fence {
    fn opened_by(line: &str) -> Option<Self> {
        let (marker, len, rest) = fence_run(line)?;
        // A backtick fence's info string cannot contain a backtick; such a line is inline code.
        if marker == b'`' && rest.contains('`') {
            return None;
        }
        Some(Self { marker, len })
    }

    fn is_closed_by(self, line: &str) -> bool {
        fence_run(line).is_some_and(|(marker, len, rest)| {
            marker == self.marker && len >= self.len && rest.trim().is_empty()
        })
    }
}

/// Splits a possible fence line, indented at most three spaces, into its marker, the marker's
/// run length, and the rest of the line.
fn fence_run(line: &str) -> Option<(u8, usize, &str)> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return None;
    }
    let body = &line[indent..];
    let marker = *body.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let len = body.bytes().take_while(|byte| *byte == marker).count();
    (len >= 3).then(|| (marker, len, &body[len..]))
}

/// The parts of `line` outside inline code spans. A span opens with a run of backticks and
/// closes at the next run of exactly as many; a run that never closes is literal text.
fn outside_code_spans(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut segments = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'`' {
            at += 1;
            continue;
        }
        let run = backtick_run(bytes, at);
        match closing_run(bytes, at + run, run) {
            Some(close) => {
                segments.push(&line[start..at]);
                at = close + run;
                start = at;
            }
            None => at += run,
        }
    }
    segments.push(&line[start..]);
    segments
}

fn backtick_run(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

fn closing_run(bytes: &[u8], mut at: usize, run: usize) -> Option<usize> {
    while at < bytes.len() {
        if bytes[at] == b'`' {
            let len = backtick_run(bytes, at);
            if len == run {
                return Some(at);
            }
            at += len;
        } else {
            at += 1;
        }
    }
    None
}

/// Appends the links in one code-free segment of line `line`.
fn links_in(segment: &str, line: usize, links: &mut Vec<Link>) {
    let mut rest = segment;
    while let Some(open) = rest.find("[[") {
        let inner_start = open + 2;
        let Some(close) = rest[inner_start..].find("]]") else {
            return;
        };
        let inner = &rest[inner_start..inner_start + close];
        if inner.contains(['[', ']']) {
            // Not a link here; a link may still start at the next bracket.
            rest = &rest[open + 1..];
            continue;
        }
        let target = inner.trim();
        if !target.is_empty() {
            links.push(Link {
                target: target.to_owned(),
                line,
            });
        }
        rest = &rest[inner_start + close + 2..];
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn targets(text: &str, skip: usize) -> Vec<(String, usize)> {
        extract(text, skip)
            .into_iter()
            .map(|link| (link.target, link.line))
            .collect()
    }

    fn found(pairs: &[(&str, usize)]) -> Vec<(String, usize)> {
        pairs
            .iter()
            .map(|(target, line)| ((*target).to_owned(), *line))
            .collect()
    }

    #[test]
    fn finds_links_with_their_lines() {
        assert_eq!(
            targets("a [[one]] b [[two]]\n\n[[three]]\n", 0),
            found(&[("one", 1), ("two", 1), ("three", 3)])
        );
    }

    #[test]
    fn skips_the_frontmatter_lines() {
        assert_eq!(
            targets("---\nname: [[not-a-link]]\n---\n[[a]]\n", 3),
            found(&[("a", 4)])
        );
    }

    #[test]
    fn whitespace_inside_brackets_is_trimmed() {
        assert_eq!(targets("[[ spaced ]]", 0), found(&[("spaced", 1)]));
    }

    #[test]
    fn empty_and_bracketed_targets_are_not_links() {
        assert_eq!(
            targets("[[]] [[ ]] [[a]b]] [[a [[b]] [[[c]]]", 0),
            found(&[("b", 1), ("c", 1)])
        );
    }

    #[test]
    fn links_in_inline_code_are_ignored() {
        assert_eq!(
            targets("use `[[name]]` syntax and [[real]]", 0),
            found(&[("real", 1)])
        );
        assert_eq!(
            targets("``code ` [[no]]`` [[yes]]", 0),
            found(&[("yes", 1)])
        );
        assert_eq!(targets("[[a `b` c]]", 0), found(&[]));
    }

    #[test]
    fn an_unclosed_backtick_run_is_literal() {
        assert_eq!(targets("a ` b [[yes]]", 0), found(&[("yes", 1)]));
    }

    #[test]
    fn links_in_fenced_blocks_are_ignored() {
        assert_eq!(
            targets("```rust\n[[no]]\n```\n[[yes]]\n", 0),
            found(&[("yes", 4)])
        );
    }

    #[test]
    fn a_fence_closes_only_on_a_bare_run_of_its_marker_at_least_as_long() {
        let text = "~~~~\n[[no]]\n~~~\n[[still-no]]\n```\n[[also-no]]\n~~~~~ x\n[[nor-this]]\n~~~~~\n[[yes]]\n";
        assert_eq!(targets(text, 0), found(&[("yes", 10)]));
    }

    #[test]
    fn a_fence_may_be_indented_up_to_three_spaces() {
        assert_eq!(
            targets("   ```\n[[no]]\n   ```\n    ```\n[[yes]]\n", 0),
            found(&[("yes", 5)])
        );
    }

    #[test]
    fn an_unclosed_fence_hides_the_rest_of_the_note() {
        assert_eq!(targets("```\n[[no]]\n", 0), found(&[]));
    }

    #[test]
    fn only_a_backtick_fence_forbids_backticks_after_the_marker() {
        assert_eq!(
            targets("``` [[no]] ```\n[[yes]]\n", 0),
            found(&[("yes", 2)])
        );
        assert_eq!(
            targets("~~~ `info`\n[[no]]\n~~~\n[[yes]]\n", 0),
            found(&[("yes", 4)])
        );
    }
}
