//! Diff presentation for tool invocations and interactive edits: line-number
//! resolution, token-level LCS diffs, and terminal rendering.

use crate::ui::theme::Theme;

// ---------------------------------------------------------------------------
// Line number resolution for edit diffs
// ---------------------------------------------------------------------------

/// Attempts to locate the 1-based start line of an edit replacement in a file.
///
/// It checks `old_text` first (prior to the edit being applied on disk), and falls
/// back to `new_text` (if the edit has already been applied to the file on disk).
pub fn find_edit_line_number(path_str: &str, old_text: &str, new_text: &str) -> Option<usize> {
    let content = std::fs::read_to_string(path_str).ok()?;
    locate_match_line(&content, old_text).or_else(|| locate_match_line(&content, new_text))
}

fn locate_match_line(content: &str, target: &str) -> Option<usize> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(idx) = content.find(target) {
        return Some(1 + content[..idx].matches('\n').count());
    }
    let norm_content = content.replace("\r\n", "\n");
    let norm_target = target.replace("\r\n", "\n");
    if let Some(idx) = norm_content.find(&norm_target) {
        return Some(1 + norm_content[..idx].matches('\n').count());
    }
    let first_line = target.lines().find(|l| !l.trim().is_empty())?;
    if content.matches(first_line).count() == 1 {
        let idx = content.find(first_line)?;
        return Some(1 + content[..idx].matches('\n').count());
    }
    None
}

// ---------------------------------------------------------------------------
// Tokenizer and LCS diff computation for inline word diffs
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum DiffToken<'a> {
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

fn tokenize(text: &str) -> Vec<&str> {
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

struct LcsTable {
    stride: usize,
    data: Vec<usize>,
}

impl LcsTable {
    #[inline(always)]
    fn get(&self, i: usize, j: usize) -> usize {
        self.data[i * self.stride + j]
    }
}

fn build_lcs_table(old_tokens: &[&str], new_tokens: &[&str]) -> LcsTable {
    let (n, m) = (old_tokens.len(), new_tokens.len());
    let stride = m + 1;
    let mut data = vec![0_usize; (n + 1) * stride];
    for i in 0..n {
        for j in 0..m {
            data[(i + 1) * stride + (j + 1)] = if old_tokens[i] == new_tokens[j] {
                data[i * stride + j] + 1
            } else {
                data[(i + 1) * stride + j].max(data[i * stride + (j + 1)])
            };
        }
    }
    LcsTable { stride, data }
}

fn backtrack_token_step<'a>(
    (old_tokens, new_tokens): (&[&'a str], &[&'a str]),
    table: &LcsTable,
    (i, j): (&mut usize, &mut usize),
) -> DiffToken<'a> {
    if *i > 0 && *j > 0 && old_tokens[*i - 1] == new_tokens[*j - 1] {
        *i -= 1;
        *j -= 1;
        DiffToken::Same(old_tokens[*i])
    } else if *j > 0 && (*i == 0 || table.get(*i, *j - 1) >= table.get(*i - 1, *j)) {
        *j -= 1;
        DiffToken::Added(new_tokens[*j])
    } else {
        *i -= 1;
        DiffToken::Removed(old_tokens[*i])
    }
}

fn compute_token_diff<'a>(old_tokens: &[&'a str], new_tokens: &[&'a str]) -> Vec<DiffToken<'a>> {
    let table = build_lcs_table(old_tokens, new_tokens);
    let (mut i, mut j) = (old_tokens.len(), new_tokens.len());
    let mut diff = Vec::new();
    while i > 0 || j > 0 {
        diff.push(backtrack_token_step((old_tokens, new_tokens), &table, (&mut i, &mut j)));
    }
    diff.reverse();
    diff
}

// ---------------------------------------------------------------------------
// Single-line word diff rendering with inversion highlights
// ---------------------------------------------------------------------------

pub fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
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
    removed_buf.push_str("\x1b[0m\n");
    added_buf.push_str("\x1b[0m\n");
    (removed_buf, added_buf)
}

// ---------------------------------------------------------------------------
// Entry diff presentation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct EntryDiffInput<'a> {
    pub idx: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub theme: &'a Theme,
    pub start_line: Option<usize>,
}

fn push_edit_header(out: &mut String, idx: usize, start_line: Option<usize>, dim: anstyle::Style) {
    if idx == 0 {
        return;
    }
    if let Some(line) = start_line {
        out.push_str(&format!("{dim}@@ edit #{} · line {line} @@{dim:#}\n", idx + 1));
    } else {
        out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", idx + 1));
    }
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
        let dim = theme.dimmed;
        out.push_str(&format!("{dim}{line:>gutter_width$} │ {dim:#}{removed}"));
        out.push_str(&format!("{dim}{line:>gutter_width$} │ {dim:#}{added}"));
    } else {
        out.push_str(&removed);
        out.push_str(&added);
    }
}

fn push_diff_lines(
    out: &mut String,
    lines: &[&str],
    is_add: bool,
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    let (prefix, color) = if is_add {
        ('+', theme.tool_ok)
    } else {
        ('-', theme.tool_err)
    };
    let dim = theme.dimmed;
    for (offset, line) in lines.iter().take(8).enumerate() {
        let clean = replace_tabs(line);
        if let Some(start) = start_line {
            let line_num = start + offset;
            out.push_str(&format!(
                "{dim}{line_num:>gutter_width$} │ {dim:#}{color}{prefix} {clean}{color:#}\n"
            ));
        } else {
            out.push_str(&format!("{color}{prefix} {clean}{color:#}\n"));
        }
    }
    if lines.len() > 8 {
        out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", lines.len() - 8));
    }
}

fn push_multi_line_diff(
    out: &mut String,
    old_lines: &[&str],
    new_lines: &[&str],
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    push_diff_lines(out, old_lines, false, start_line, gutter_width, theme);
    push_diff_lines(out, new_lines, true, start_line, gutter_width, theme);
}

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    push_edit_header(&mut out, input.idx, input.start_line, input.theme.dimmed);

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();
    let max_line = input
        .start_line
        .map(|start| start + old_lines.len().max(new_lines.len()))
        .unwrap_or(0);
    let gutter_width = max_line.to_string().len().max(3);

    if old_lines.len() == 1 && new_lines.len() == 1 {
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
            input.start_line,
            gutter_width,
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
