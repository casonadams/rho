//! Diff presentation for tool invocations and interactive edits: line-number
//! resolution, token-level LCS diffs, and terminal rendering.

use crate::ui::theme::{SGR_RESET, Theme};

// ---------------------------------------------------------------------------
// Tokenizer and LCS diff computation for inline word diffs
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DiffToken<'a> {
    Same(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
enum CharCat {
    Whitespace,
    Alphanumeric,
    Other,
}

fn char_category(c: char) -> CharCat {
    if c.is_whitespace() {
        CharCat::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharCat::Alphanumeric
    } else {
        CharCat::Other
    }
}

pub(crate) fn tokenize(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if let Some(&(_, next_c)) = chars.peek() {
            if char_category(c) != char_category(next_c) {
                let end = idx + c.len_utf8();
                tokens.push(&text[start..end]);
                start = end;
            }
        } else {
            tokens.push(&text[start..]);
        }
    }
    tokens
}

fn handle_diff_op<'a>(
    op: similar::DiffOp,
    old_tokens: &[&'a str],
    new_tokens: &[&'a str],
    diff: &mut Vec<DiffToken<'a>>,
) {
    match op {
        similar::DiffOp::Equal { old_index, len, .. } => {
            diff.extend(
                old_tokens[old_index..old_index + len]
                    .iter()
                    .copied()
                    .map(DiffToken::Same),
            );
        }
        similar::DiffOp::Delete { old_index, old_len, .. } => {
            diff.extend(
                old_tokens[old_index..old_index + old_len]
                    .iter()
                    .copied()
                    .map(DiffToken::Removed),
            );
        }
        similar::DiffOp::Insert { new_index, new_len, .. } => {
            diff.extend(
                new_tokens[new_index..new_index + new_len]
                    .iter()
                    .copied()
                    .map(DiffToken::Added),
            );
        }
        similar::DiffOp::Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        } => {
            diff.extend(
                old_tokens[old_index..old_index + old_len]
                    .iter()
                    .copied()
                    .map(DiffToken::Removed),
            );
            diff.extend(
                new_tokens[new_index..new_index + new_len]
                    .iter()
                    .copied()
                    .map(DiffToken::Added),
            );
        }
    }
}

/// Longest pair of lines, in bytes, that is diffed word by word; longer pairs show as whole lines.
const MAX_WORD_DIFF_BYTES: usize = 16 * 1024;

/// How long a diff may search for the shortest edit before settling for a coarser one.
const DIFF_TIME_LIMIT: std::time::Duration = std::time::Duration::from_millis(30);

pub(crate) fn fits_word_diff(old_line: &str, new_line: &str) -> bool {
    old_line.len() + new_line.len() <= MAX_WORD_DIFF_BYTES
}

pub(crate) fn capture_diff<T: std::hash::Hash + Eq>(old: &[T], new: &[T]) -> Vec<similar::DiffOp> {
    let deadline = std::time::Instant::now() + DIFF_TIME_LIMIT;
    similar::capture_diff_slices_deadline(similar::Algorithm::Myers, old, new, Some(deadline))
}

pub(crate) fn compute_token_diff<'a>(old_tokens: &[&'a str], new_tokens: &[&'a str]) -> Vec<DiffToken<'a>> {
    let mut diff = Vec::new();
    for op in capture_diff(old_tokens, new_tokens) {
        handle_diff_op(op, old_tokens, new_tokens, &mut diff);
    }
    diff
}

// ---------------------------------------------------------------------------
// Single-line word diff rendering with inversion highlights
// ---------------------------------------------------------------------------

pub fn replace_tabs(text: &str) -> String {
    text.replace('\t', "    ")
}

fn split_leading_whitespace(token: &str) -> (&str, &str) {
    let non_ws_idx = token.find(|c: char| !c.is_whitespace()).unwrap_or(token.len());
    (&token[..non_ws_idx], &token[non_ws_idx..])
}

fn push_inverted_token(buf: &mut String, text: &str) {
    let (ws, non_ws) = split_leading_whitespace(text);
    buf.push_str(ws);
    if !non_ws.is_empty() {
        buf.push_str("\x1b[7m");
        buf.push_str(non_ws);
        buf.push_str("\x1b[27m");
    }
}

fn append_diff_token(token: DiffToken<'_>, removed: &mut String, added: &mut String) {
    match token {
        DiffToken::Same(text) => {
            removed.push_str(text);
            added.push_str(text);
        }
        DiffToken::Removed(text) => push_inverted_token(removed, text),
        DiffToken::Added(text) => push_inverted_token(added, text),
    }
}

pub fn render_single_line_word_diff(old_line: &str, new_line: &str, theme: &Theme) -> (String, String) {
    let clean_old = replace_tabs(old_line);
    let clean_new = replace_tabs(new_line);
    let old_tokens = tokenize(&clean_old);
    let new_tokens = tokenize(&clean_new);
    let diff = compute_token_diff(&old_tokens, &new_tokens);

    let (red, green) = (theme.tool_err, theme.tool_ok);
    let (mut removed_buf, mut added_buf) = (format!("{red}- "), format!("{green}+ "));
    for token in diff {
        append_diff_token(token, &mut removed_buf, &mut added_buf);
    }
    removed_buf.push_str(&format!("{SGR_RESET}\n"));
    added_buf.push_str(&format!("{SGR_RESET}\n"));
    (removed_buf, added_buf)
}

// ---------------------------------------------------------------------------
// Entry diff presentation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct EntryDiffInput<'a> {
    pub idx: usize,
    pub total: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub theme: &'a Theme,
    pub start_line: Option<usize>,
    pub lang: Option<&'a str>,
}

fn push_edit_header(out: &mut String, idx: usize, total: usize, start_line: Option<usize>, dim: anstyle::Style) {
    if total < 2 {
        return;
    }
    let number = idx + 1;
    match start_line {
        Some(line) => out.push_str(&format!(
            "{dim}@@ edit #{number}/{total} \u{b7} line {line} @@{dim:#}\n"
        )),
        None => out.push_str(&format!("{dim}@@ edit #{number}/{total} @@{dim:#}\n")),
    }
}

pub fn format_gutter_prefix(line_num: usize, gutter_width: usize, dim: anstyle::Style) -> String {
    format!("{dim}{line_num:>gutter_width$} │ {dim:#}")
}

fn push_single_line_diff(
    out: &mut String,
    old_line: &str,
    new_line: &str,
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    let (removed, added) = render_single_line_word_diff(old_line, new_line, theme);
    if let Some(line) = start_line {
        let prefix = format_gutter_prefix(line, gutter_width, theme.dimmed);
        out.push_str(&format!("{prefix}{removed}"));
        out.push_str(&format!("{prefix}{added}"));
    } else {
        out.push_str(&removed);
        out.push_str(&added);
    }
}

struct DiffSide<'a> {
    lines: &'a [&'a str],
    is_add: bool,
}

#[derive(Clone, Copy)]
struct DiffLayout {
    start_line: Option<usize>,
    gutter_width: usize,
}

fn push_diff_lines(out: &mut String, side: DiffSide<'_>, layout: DiffLayout, theme: &Theme) {
    let DiffSide { lines, is_add } = side;
    let DiffLayout {
        start_line,
        gutter_width,
    } = layout;
    let (prefix, color) = if is_add {
        ('+', theme.tool_ok)
    } else {
        ('-', theme.tool_err)
    };
    let dim = theme.dimmed;
    for (offset, line) in lines.iter().enumerate() {
        let clean = replace_tabs(line);
        if let Some(start) = start_line {
            let line_num = start + offset;
            let gutter = format_gutter_prefix(line_num, gutter_width, dim);
            out.push_str(&format!("{gutter}{color}{prefix} {clean}{color:#}\n"));
        } else {
            out.push_str(&format!("{color}{prefix} {clean}{color:#}\n"));
        }
    }
}

fn push_multi_line_diff(out: &mut String, old_lines: &[&str], new_lines: &[&str], layout: DiffLayout, theme: &Theme) {
    let old_side = DiffSide {
        lines: old_lines,
        is_add: false,
    };
    let new_side = DiffSide {
        lines: new_lines,
        is_add: true,
    };
    push_diff_lines(out, old_side, layout, theme);
    push_diff_lines(out, new_side, layout, theme);
}

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    push_edit_header(&mut out, input.idx, input.total, input.start_line, input.theme.dimmed);

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();
    let max_line = input
        .start_line
        .map(|start| start + old_lines.len().max(new_lines.len()))
        .unwrap_or(0);
    let gutter_width = max_line.to_string().len().max(3);

    if input.theme.delta_diffs {
        super::delta_diff::render_delta_diff(
            &mut out,
            super::delta_diff::DeltaDiffRequest {
                old_lines: &old_lines,
                new_lines: &new_lines,
                lang: input.lang,
                start_line: input.start_line,
                gutter_width,
                theme: input.theme,
            },
        );
        return out;
    }

    if old_lines.len() == 1 && new_lines.len() == 1 && fits_word_diff(old_lines[0], new_lines[0]) {
        push_single_line_diff(
            &mut out,
            old_lines[0],
            new_lines[0],
            input.start_line,
            gutter_width,
            input.theme,
        );
    } else {
        push_multi_line_diff(
            &mut out,
            &old_lines,
            &new_lines,
            DiffLayout {
                start_line: input.start_line,
                gutter_width,
            },
            input.theme,
        );
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_handles_multibyte_unicode_without_panicking() {
        let text = "### Slice 1: File Transclusion — **2 pts**";
        let tokens = tokenize(text);
        assert!(!tokens.is_empty());
        let joined = tokens.concat();
        assert_eq!(joined, text);
    }

    #[test]
    fn tokenize_handles_emojis_and_cjk_characters() {
        let text = "🦀 emoji and 中文 text — with dashes";
        let tokens = tokenize(text);
        let joined = tokens.concat();
        assert_eq!(joined, text);
    }

    #[test]
    fn tokenize_handles_empty_and_single_char() {
        assert_eq!(tokenize(""), Vec::<&str>::new());
        assert_eq!(tokenize("a"), vec!["a"]);
        assert_eq!(tokenize("—"), vec!["—"]);
        assert_eq!(tokenize("🦀"), vec!["🦀"]);
    }

    #[test]
    fn single_line_word_diff_with_unicode_does_not_panic() {
        let theme = Theme::default();
        let old_line = "### Slice 1: File Transclusion — **2 pts** (Draft)";
        let new_line = "### Slice 1: File Transclusion — **2 pts** (Complete)";

        let (removed, added) = render_single_line_word_diff(old_line, new_line, &theme);
        assert!(removed.contains("Draft"));
        assert!(added.contains("Complete"));
        assert!(removed.contains("—"));
        assert!(added.contains("—"));
    }

    #[test]
    fn single_line_word_diff_terminates_with_full_sgr_reset() {
        let theme = Theme::default();
        let (removed, added) = render_single_line_word_diff("let old = 1;", "let new = 1;", &theme);
        assert!(removed.ends_with("\x1b[0m\n"));
        assert!(added.ends_with("\x1b[0m\n"));

        let unstyled_theme = Theme {
            tool_err: anstyle::Style::new(),
            tool_ok: anstyle::Style::new(),
            ..Default::default()
        };
        let (removed, added) = render_single_line_word_diff("let val = old", "let val = new", &unstyled_theme);
        assert!(removed.ends_with("\x1b[0m\n"));
        assert!(added.ends_with("\x1b[0m\n"));
    }

    fn minified_pair() -> (String, String) {
        let old: String = (0..6_000).map(|i| format!("a{i}=f({i},b{i});")).collect();
        let new: String = (0..6_000).map(|i| format!("a{i}=g({},b{i});", i * 7 % 13)).collect();
        (old, new)
    }

    #[test]
    fn a_huge_single_line_edit_is_shown_as_whole_removed_and_added_lines() {
        let theme = Theme::default();
        let (old, new) = minified_pair();
        let input = EntryDiffInput {
            idx: 0,
            total: 1,
            old_text: &old,
            new_text: &new,
            theme: &theme,
            start_line: Some(1),
            lang: Some("js"),
        };
        let mut expected = String::new();
        let layout = DiffLayout {
            start_line: Some(1),
            gutter_width: 3,
        };
        push_multi_line_diff(&mut expected, &[&old], &[&new], layout, &theme);
        assert_eq!(format_entry_diff(input), expected);
    }

    #[test]
    fn compute_token_diff_matches_identical_tokens() {
        let old_tokens = vec!["hello", " ", "world"];
        let new_tokens = vec!["hello", " ", "world"];
        let diff = compute_token_diff(&old_tokens, &new_tokens);
        assert_eq!(
            diff,
            vec![DiffToken::Same("hello"), DiffToken::Same(" "), DiffToken::Same("world"),]
        );
    }

    #[test]
    fn compute_token_diff_handles_add_and_remove() {
        let old_tokens = vec!["old"];
        let new_tokens = vec!["new"];
        let diff = compute_token_diff(&old_tokens, &new_tokens);
        assert_eq!(diff, vec![DiffToken::Removed("old"), DiffToken::Added("new")]);
    }
}
