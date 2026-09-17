//! Frontmatter: the `---` block at the top of a note.
//!
//! The format is a small, line-based relative of YAML: top-level `key: value` lines, plus one
//! level of nesting under a key with no value (`metadata:` followed by indented `key: value`
//! lines). A value wrapped in single or double quotes is unquoted; any other value is taken
//! literally to the end of the line, so unlike YAML a `#` or `: ` inside it needs no quoting.
//! Block scalars, flow collections, and deeper nesting are syntax errors rather than guesses.

use std::collections::BTreeMap;

/// A parsed frontmatter block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    /// Top-level keys, sorted.
    pub entries: BTreeMap<String, Entry>,
    /// Number of lines the block occupies, both delimiters included. The body starts after them.
    pub body_start: usize,
}

/// One top-level key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 1-based line of the key.
    pub line: usize,
    /// The key's value.
    pub value: Value,
}

/// A frontmatter value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A single-line value, possibly empty.
    Scalar(String),
    /// Nested `key: value` lines, each with its 1-based line.
    Map(BTreeMap<String, (usize, String)>),
}

/// Why a note has no usable frontmatter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontmatterError {
    /// The note does not start with a `---` line.
    Missing,
    /// The opening `---` has no closing `---`.
    Unterminated,
    /// A line inside the block is outside the accepted syntax.
    Syntax {
        /// 1-based line of the problem.
        line: usize,
        /// What is wrong.
        message: String,
        /// Where the body starts, as in [`Frontmatter::body_start`].
        body_start: usize,
    },
}

/// Parses the frontmatter block at the top of `text`.
///
/// # Errors
///
/// Returns [`FrontmatterError`] when the block is missing, unterminated, or malformed. Only the
/// first syntax error is reported.
pub fn parse(text: &str) -> Result<Frontmatter, FrontmatterError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    if lines.first().map(|line| line.trim_end()) != Some(DELIMITER) {
        return Err(FrontmatterError::Missing);
    }
    let Some(close) = lines
        .iter()
        .skip(1)
        .position(|line| line.trim_end() == DELIMITER)
    else {
        return Err(FrontmatterError::Unterminated);
    };
    // `close` counts from the second line, so the closing delimiter is line `close + 2`.
    let body_start = close + 2;
    let mut parser = Parser::default();
    for (index, line) in lines.iter().enumerate().take(body_start - 1).skip(1) {
        parser
            .line(index + 1, line)
            .map_err(|message| FrontmatterError::Syntax {
                line: index + 1,
                message,
                body_start,
            })?;
    }
    Ok(Frontmatter {
        entries: parser.finish(),
        body_start,
    })
}

const DELIMITER: &str = "---";

/// Parser state: the finished top-level entries, plus the key whose nested lines are being read.
#[derive(Default)]
struct Parser {
    entries: BTreeMap<String, Entry>,
    open: Option<OpenMap>,
}

/// A top-level key with no value, collecting the indented lines under it.
struct OpenMap {
    key: String,
    line: usize,
    indent: Option<usize>,
    children: BTreeMap<String, (usize, String)>,
}

impl Parser {
    fn line(&mut self, number: usize, line: &str) -> Result<(), String> {
        let content = line.trim_end();
        let body = content.trim_start_matches([' ', '\t']);
        if body.is_empty() || body.starts_with('#') {
            return Ok(());
        }
        let lead = &content[..content.len() - body.len()];
        if lead.contains('\t') {
            return Err("tab in indentation; indent with spaces".to_owned());
        }
        if lead.is_empty() {
            self.top_level(number, body)
        } else {
            self.nested(number, lead.len(), body)
        }
    }

    fn top_level(&mut self, number: usize, body: &str) -> Result<(), String> {
        self.close_map();
        let (key, raw) = split_key(body)?;
        if self.entries.contains_key(key) {
            return Err(format!("duplicate key `{key}`"));
        }
        if raw.is_empty() {
            self.open = Some(OpenMap {
                key: key.to_owned(),
                line: number,
                indent: None,
                children: BTreeMap::new(),
            });
        } else {
            let value = Value::Scalar(parse_value(raw)?);
            self.entries.insert(
                key.to_owned(),
                Entry {
                    line: number,
                    value,
                },
            );
        }
        Ok(())
    }

    fn nested(&mut self, number: usize, indent: usize, body: &str) -> Result<(), String> {
        let Some(open) = self.open.as_mut() else {
            return Err("indented line without a parent key".to_owned());
        };
        match open.indent {
            None => open.indent = Some(indent),
            Some(expected) if expected != indent => {
                return Err(format!(
                    "inconsistent indentation: expected {expected} spaces, found {indent}; only one level of nesting is supported"
                ));
            }
            Some(_) => {}
        }
        let (key, raw) = split_key(body)?;
        if open.children.contains_key(key) {
            return Err(format!("duplicate key `{}.{key}`", open.key));
        }
        let value = if raw.is_empty() {
            String::new()
        } else {
            parse_value(raw)?
        };
        open.children.insert(key.to_owned(), (number, value));
        Ok(())
    }

    fn close_map(&mut self) {
        if let Some(open) = self.open.take() {
            let value = if open.children.is_empty() {
                Value::Scalar(String::new())
            } else {
                Value::Map(open.children)
            };
            self.entries.insert(
                open.key,
                Entry {
                    line: open.line,
                    value,
                },
            );
        }
    }

    fn finish(mut self) -> BTreeMap<String, Entry> {
        self.close_map();
        self.entries
    }
}

/// Splits `key: value` into the key and the trimmed raw value.
fn split_key(body: &str) -> Result<(&str, &str), String> {
    let key_len = body
        .bytes()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        .count();
    let (key, rest) = body.split_at(key_len);
    let Some(rest) = rest.strip_prefix(':').filter(|_| !key.is_empty()) else {
        return Err("expected `key: value`".to_owned());
    };
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return Err("expected a space after `:`".to_owned());
    }
    Ok((key, rest.trim()))
}

/// Parses a non-empty raw value.
fn parse_value(raw: &str) -> Result<String, String> {
    if let Some(quoted) = raw.strip_prefix('"') {
        double_quoted(quoted)
    } else if let Some(quoted) = raw.strip_prefix('\'') {
        single_quoted(quoted)
    } else if raw.starts_with(['|', '>']) {
        Err("block scalars are not supported; keep the value on one line".to_owned())
    } else if raw.starts_with(['[', '{']) {
        Err("flow collections are not supported; quote the value to keep it as text".to_owned())
    } else {
        Ok(raw.to_owned())
    }
}

/// Unquotes the rest of a double-quoted value, where only `\"` and `\\` are escapes.
fn double_quoted(rest: &str) -> Result<String, String> {
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return after_closing_quote(chars.as_str(), value),
            '\\' => match chars.next() {
                Some(escaped @ ('"' | '\\')) => value.push(escaped),
                Some(other) => return Err(format!("unsupported escape `\\{other}`")),
                None => break,
            },
            other => value.push(other),
        }
    }
    Err("unterminated quoted value".to_owned())
}

/// Unquotes the rest of a single-quoted value, where `''` stands for `'`.
fn single_quoted(mut rest: &str) -> Result<String, String> {
    let mut value = String::new();
    while let Some(quote) = rest.find('\'') {
        value.push_str(&rest[..quote]);
        rest = &rest[quote + 1..];
        match rest.strip_prefix('\'') {
            Some(after) => {
                value.push('\'');
                rest = after;
            }
            None => return after_closing_quote(rest, value),
        }
    }
    Err("unterminated quoted value".to_owned())
}

fn after_closing_quote(rest: &str, value: String) -> Result<String, String> {
    if rest.is_empty() {
        Ok(value)
    } else {
        Err("unexpected text after the closing quote".to_owned())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn scalar(frontmatter: &Frontmatter, key: &str) -> (usize, String) {
        match &frontmatter.entries[key] {
            Entry {
                line,
                value: Value::Scalar(value),
            } => (*line, value.clone()),
            other => panic!("`{key}` is not a scalar: {other:?}"),
        }
    }

    fn map(frontmatter: &Frontmatter, key: &str) -> (usize, BTreeMap<String, (usize, String)>) {
        match &frontmatter.entries[key] {
            Entry {
                line,
                value: Value::Map(map),
            } => (*line, map.clone()),
            other => panic!("`{key}` is not a map: {other:?}"),
        }
    }

    fn syntax_error(text: &str) -> (usize, String, usize) {
        match parse(text) {
            Err(FrontmatterError::Syntax {
                line,
                message,
                body_start,
            }) => (line, message, body_start),
            other => panic!("expected a syntax error, got {other:?}"),
        }
    }

    #[test]
    fn parses_scalars_and_one_level_of_nesting() {
        let text = "---\nname: fix-flaky-test\nmetadata:\n  type: feedback\n  origin: review\ndescription: How the flaky test was fixed\n---\nbody\n";
        let frontmatter = parse(text).unwrap();
        assert_eq!(
            scalar(&frontmatter, "name"),
            (2, "fix-flaky-test".to_owned())
        );
        let (line, metadata) = map(&frontmatter, "metadata");
        assert_eq!(line, 3);
        assert_eq!(metadata["type"], (4, "feedback".to_owned()));
        assert_eq!(metadata["origin"], (5, "review".to_owned()));
        assert_eq!(
            scalar(&frontmatter, "description"),
            (6, "How the flaky test was fixed".to_owned())
        );
        assert_eq!(frontmatter.entries.len(), 3);
        assert_eq!(frontmatter.body_start, 7);
    }

    #[test]
    fn a_note_without_an_opening_delimiter_has_no_frontmatter() {
        assert_eq!(
            parse("# Title\n---\nname: a\n---\n"),
            Err(FrontmatterError::Missing)
        );
        assert_eq!(parse(""), Err(FrontmatterError::Missing));
    }

    #[test]
    fn an_unclosed_block_is_unterminated() {
        assert_eq!(parse("---\nname: a\n"), Err(FrontmatterError::Unterminated));
        assert_eq!(parse("---"), Err(FrontmatterError::Unterminated));
    }

    #[test]
    fn quoted_values_are_unquoted() {
        let text = "---\nname: \"a-b\"\ndescription: 'it''s \"here\"'\nsummary: \"say \\\"hi\\\" \\\\ ok\"\n---\n";
        let frontmatter = parse(text).unwrap();
        assert_eq!(scalar(&frontmatter, "name").1, "a-b");
        assert_eq!(scalar(&frontmatter, "description").1, "it's \"here\"");
        assert_eq!(scalar(&frontmatter, "summary").1, "say \"hi\" \\ ok");
    }

    #[test]
    fn plain_values_are_literal_to_the_end_of_the_line() {
        let frontmatter = parse("---\ndescription: see #12: done  \n---\n").unwrap();
        assert_eq!(scalar(&frontmatter, "description").1, "see #12: done");
    }

    #[test]
    fn keys_without_values_are_empty() {
        let frontmatter = parse("---\nname:\nmetadata:\n  type:\ndescription: ''\n---\n").unwrap();
        assert_eq!(scalar(&frontmatter, "name"), (2, String::new()));
        assert_eq!(map(&frontmatter, "metadata").1["type"], (4, String::new()));
        assert_eq!(scalar(&frontmatter, "description"), (5, String::new()));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let text =
            "---\n# a comment\n\nname: a\nmetadata:\n  # a nested comment\n\n  type: user\n---\n";
        let frontmatter = parse(text).unwrap();
        assert_eq!(scalar(&frontmatter, "name"), (4, "a".to_owned()));
        assert_eq!(
            map(&frontmatter, "metadata"),
            (
                5,
                BTreeMap::from([("type".to_owned(), (8, "user".to_owned()))])
            )
        );
        assert_eq!(frontmatter.body_start, 9);
    }

    #[test]
    fn crlf_line_endings_and_a_byte_order_mark_are_accepted() {
        let frontmatter = parse("\u{feff}---\r\nname: a\r\n---\r\nbody\r\n").unwrap();
        assert_eq!(scalar(&frontmatter, "name"), (2, "a".to_owned()));
        assert_eq!(frontmatter.body_start, 3);
    }

    #[test]
    fn duplicate_keys_are_syntax_errors() {
        let (line, message, body_start) = syntax_error("---\nname: a\nname: b\n---\n");
        assert_eq!((line, body_start), (3, 4));
        assert!(message.contains("duplicate key `name`"), "{message}");

        let (line, message, _) = syntax_error("---\nmetadata:\n  type: a\nmetadata:\n---\n");
        assert_eq!(line, 4);
        assert!(message.contains("duplicate key `metadata`"), "{message}");

        let (line, message, _) = syntax_error("---\nmetadata:\n  type: a\n  type: b\n---\n");
        assert_eq!(line, 4);
        assert!(
            message.contains("duplicate key `metadata.type`"),
            "{message}"
        );
    }

    #[test]
    fn a_line_that_is_not_key_colon_value_is_a_syntax_error() {
        for line in [
            "just text",
            "- item",
            "name:value",
            ": value",
            "name : value",
        ] {
            assert_eq!(syntax_error(&format!("---\n{line}\n---\n")).0, 2, "{line}");
        }
    }

    #[test]
    fn indentation_without_a_parent_key_is_a_syntax_error() {
        assert_eq!(syntax_error("---\n  name: a\n---\n").0, 2);
        assert_eq!(syntax_error("---\nname: a\n  type: b\n---\n").0, 3);
        assert_eq!(syntax_error("---\nname: \"\"\n  type: b\n---\n").0, 3);
    }

    #[test]
    fn nesting_deeper_than_one_level_is_a_syntax_error() {
        let (line, message, _) = syntax_error("---\nmetadata:\n  type: a\n    deeper: b\n---\n");
        assert_eq!(line, 4);
        assert!(message.contains("indentation"), "{message}");
    }

    #[test]
    fn tabs_in_indentation_are_syntax_errors() {
        let (line, message, _) = syntax_error("---\nmetadata:\n\ttype: a\n---\n");
        assert_eq!(line, 3);
        assert!(message.contains("tab"), "{message}");
    }

    #[test]
    fn unsupported_values_are_syntax_errors_that_say_why() {
        for (value, expected) in [
            ("|", "block scalars are not supported"),
            ("> folded", "block scalars are not supported"),
            ("[a, b]", "flow collections are not supported"),
            ("{a: b}", "flow collections are not supported"),
            ("\"open", "unterminated quoted value"),
            ("'open", "unterminated quoted value"),
            ("\"a\" b", "text after the closing quote"),
            ("'a' b", "text after the closing quote"),
            ("\"a\\nb\"", "unsupported escape `\\n`"),
        ] {
            let (line, message, _) = syntax_error(&format!("---\nkey: {value}\n---\n"));
            assert_eq!(line, 2, "{value}");
            assert!(message.contains(expected), "{value}: {message}");
        }
    }
}
