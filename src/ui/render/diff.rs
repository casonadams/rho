use crate::ui::theme::Theme;

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

pub fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
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
                tokens.push(&text[start..=idx]);
                start = idx + c.len_utf8();
            }
        } else {
            tokens.push(&text[start..]);
        }
    }
    tokens
}

fn compute_token_diff<'a>(old_tokens: &[&'a str], new_tokens: &[&'a str]) -> Vec<DiffToken<'a>> {
    let n = old_tokens.len();
    let m = new_tokens.len();
    let mut table = vec![vec![0_usize; m + 1]; n + 1];

    for i in 0..n {
        for j in 0..m {
            if old_tokens[i] == new_tokens[j] {
                table[i + 1][j + 1] = table[i][j] + 1;
            } else {
                table[i + 1][j + 1] = table[i + 1][j].max(table[i][j + 1]);
            }
        }
    }

    let mut i = n;
    let mut j = m;
    let mut diff = Vec::new();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old_tokens[i - 1] == new_tokens[j - 1] {
            diff.push(DiffToken::Same(old_tokens[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || table[i][j - 1] >= table[i - 1][j]) {
            diff.push(DiffToken::Added(new_tokens[j - 1]));
            j -= 1;
        } else if i > 0 && (j == 0 || table[i][j - 1] < table[i - 1][j]) {
            diff.push(DiffToken::Removed(old_tokens[i - 1]));
            i -= 1;
        }
    }

    diff.reverse();
    diff
}

fn split_leading_whitespace(token: &str) -> (&str, &str) {
    let non_ws_idx = token.find(|c: char| !c.is_whitespace()).unwrap_or(token.len());
    (&token[..non_ws_idx], &token[non_ws_idx..])
}

pub fn render_single_line_word_diff(old_line: &str, new_line: &str, theme: &Theme) -> (String, String) {
    let clean_old = replace_tabs(old_line);
    let clean_new = replace_tabs(new_line);

    let old_tokens = tokenize(&clean_old);
    let new_tokens = tokenize(&clean_new);
    let diff = compute_token_diff(&old_tokens, &new_tokens);

    let red = theme.tool_err;
    let green = theme.tool_ok;

    let mut removed_buf = format!("{red}- ");
    let mut added_buf = format!("{green}+ ");

    for token in diff {
        match token {
            DiffToken::Same(text) => {
                removed_buf.push_str(text);
                added_buf.push_str(text);
            }
            DiffToken::Removed(text) => {
                let (ws, non_ws) = split_leading_whitespace(text);
                removed_buf.push_str(ws);
                if !non_ws.is_empty() {
                    removed_buf.push_str("\x1b[7m");
                    removed_buf.push_str(non_ws);
                    removed_buf.push_str("\x1b[27m");
                }
            }
            DiffToken::Added(text) => {
                let (ws, non_ws) = split_leading_whitespace(text);
                added_buf.push_str(ws);
                if !non_ws.is_empty() {
                    added_buf.push_str("\x1b[7m");
                    added_buf.push_str(non_ws);
                    added_buf.push_str("\x1b[27m");
                }
            }
        }
    }

    removed_buf.push_str(&format!("{red:#}\n"));
    added_buf.push_str(&format!("{green:#}\n"));

    (removed_buf, added_buf)
}

#[derive(Debug, Clone, Copy)]
pub struct EntryDiffInput<'a> {
    pub idx: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub theme: &'a Theme,
}

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    if input.idx > 0 {
        let dim = input.theme.dimmed;
        out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", input.idx + 1));
    }

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();

    if old_lines.len() == 1 && new_lines.len() == 1 {
        let (removed, added) = render_single_line_word_diff(old_lines[0], new_lines[0], input.theme);
        out.push_str(&removed);
        out.push_str(&added);
    } else {
        let red = input.theme.tool_err;
        for line in old_lines.iter().take(8) {
            let clean = replace_tabs(line);
            out.push_str(&format!("{red}- {clean}{red:#}\n"));
        }
        if old_lines.len() > 8 {
            let dim = input.theme.dimmed;
            out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", old_lines.len() - 8));
        }

        let green = input.theme.tool_ok;
        for line in new_lines.iter().take(8) {
            let clean = replace_tabs(line);
            out.push_str(&format!("{green}+ {clean}{green:#}\n"));
        }
        if new_lines.len() > 8 {
            let dim = input.theme.dimmed;
            out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", new_lines.len() - 8));
        }
    }

    out
}
