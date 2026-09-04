//! Single-line word diff rendering with terminal inversion highlights.

use super::token::{DiffToken, compute_token_diff, tokenize};
use crate::ui::theme::Theme;

pub fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
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
