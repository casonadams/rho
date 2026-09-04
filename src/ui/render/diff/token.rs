//! Tokenizer and dynamic-programming diff computation for inline word diffs.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DiffToken<'a> {
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

pub(super) fn tokenize(text: &str) -> Vec<&str> {
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

pub(super) fn compute_token_diff<'a>(old_tokens: &[&'a str], new_tokens: &[&'a str]) -> Vec<DiffToken<'a>> {
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
