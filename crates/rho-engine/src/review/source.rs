//! A file prepared for review: per line, the code with string contents masked, comment and test flags.

use super::diff::ChangedFile;
use super::lang::{Lang, is_test_path};

#[derive(Debug, Clone)]
pub struct Line {
    pub no: usize,
    /// The trimmed source line.
    pub raw: String,
    /// `raw` with string literal contents removed, so rules do not match text inside strings.
    pub code: String,
    pub comment: bool,
    pub added: bool,
    pub test: bool,
    /// Leading whitespace in columns (a tab counts as one indent unit).
    pub indent: usize,
}

#[derive(Debug, Clone)]
pub struct Source {
    pub path: String,
    pub lang: Lang,
    pub is_new: bool,
    pub lines: Vec<Line>,
}

impl Source {
    #[must_use]
    pub fn from_changed(file: &ChangedFile) -> Self {
        let lines = file.lines.iter().map(|l| (l.no, l.text.as_str(), l.added));
        Self::prepare(&file.path, file.is_new, lines)
    }

    #[must_use]
    pub fn from_text(path: &str, text: &str) -> Self {
        let lines = text.lines().enumerate().map(|(i, l)| (i + 1, l, false));
        Self::prepare(path, false, lines)
    }

    /// A new file whose every line is added, for tests.
    #[cfg(test)]
    pub fn all_added(path: &str, text: &str) -> Self {
        let lines = text.lines().enumerate().map(|(i, l)| (i + 1, l, true));
        Self::prepare(path, true, lines)
    }

    fn prepare<'a>(path: &str, is_new: bool, lines: impl Iterator<Item = (usize, &'a str, bool)>) -> Self {
        let lang = Lang::from_path(path);
        let mut test = is_test_path(path);
        let mut in_string = false;
        let mut prepared = Vec::new();
        for (no, text, added) in lines {
            let raw = text.trim().to_string();
            let comment = !in_string && lang.is_comment(&raw);
            let code = if comment {
                raw.clone()
            } else {
                mask_strings(&raw, lang.line_comment(), &mut in_string)
            };
            test |= lang == Lang::Rust && !comment && raw.starts_with("#[cfg(test)]");
            prepared.push(Line {
                no,
                raw,
                code,
                comment,
                added,
                test,
                indent: indent_of(text, lang),
            });
        }
        Self {
            path: path.to_string(),
            lang,
            is_new,
            lines: prepared,
        }
    }
}

fn indent_of(text: &str, lang: Lang) -> usize {
    text.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { lang.indent_unit() } else { 1 })
        .sum()
}

pub(super) fn is_char_quote(chars: &[char], at: usize) -> bool {
    chars.get(at..at + 3) == Some(&['\'', '"', '\''])
}

fn marker_at(chars: &[char], at: usize, marker: Option<&str>) -> bool {
    marker.is_some_and(|m| m.chars().enumerate().all(|(i, c)| chars.get(at + i) == Some(&c)))
}

/// Index after the closing quote at or beyond `at`; clears `in_string` when it closes.
fn skip_string_body(chars: &[char], at: usize, in_string: &mut bool) -> usize {
    let mut i = at;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '"' => {
                *in_string = false;
                return i + 1;
            }
            _ => i += 1,
        }
    }
    chars.len()
}

/// Drops the contents of double-quoted strings, which may continue onto later lines (`in_string`).
fn mask_strings(text: &str, comment_marker: Option<&str>, in_string: &mut bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if *in_string {
            i = skip_string_body(&chars, i, in_string);
            if !*in_string {
                out.push('"');
            }
        } else if marker_at(&chars, i, comment_marker) {
            out.extend(&chars[i..]);
            break;
        } else if is_char_quote(&chars, i) {
            out.extend(&chars[i..i + 3]);
            i += 3;
        } else {
            *in_string = chars[i] == '"';
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code_of(path: &str, text: &str) -> Vec<String> {
        Source::from_text(path, text)
            .lines
            .into_iter()
            .map(|l| l.code)
            .collect()
    }

    #[test]
    fn string_contents_are_masked_but_comments_survive() {
        let code = code_of("a.rs", "let a = \"dbg!(x)\"; // TODO \"q\"\nlet c = '\"'; dbg!(c);\n");
        assert_eq!(code, ["let a = \"\"; // TODO \"q\"", "let c = '\"'; dbg!(c);"]);
    }

    #[test]
    fn multi_line_strings_are_masked_across_lines() {
        let code = code_of("a.rs", "let s = \"one\ndbg!(two)\nthree\"; dbg!(x);\n");
        assert_eq!(code, ["let s = \"", "", "\"; dbg!(x);"]);
    }

    #[test]
    fn rust_code_after_cfg_test_is_test_code() {
        let source = Source::from_text("a.rs", "fn a() {}\n#[cfg(test)]\nmod tests {\n}\n");
        let flags: Vec<bool> = source.lines.iter().map(|l| l.test).collect();
        assert_eq!(flags, [false, true, true, true]);
        assert!(Source::from_text("tests/a.rs", "fn a() {}").lines[0].test);
    }

    #[test]
    fn indentation_counts_tabs_as_units() {
        let source = Source::from_text("a.go", "\t\tx\n    y\n");
        assert_eq!((source.lines[0].indent, source.lines[1].indent), (8, 4));
    }
}
