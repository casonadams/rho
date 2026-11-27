//! Credential shapes shared by memory admission (which refuses them) and session redaction (which masks them).

pub(crate) const MIN_SECRET_LEN: usize = 4;
pub(crate) const REDACTED_MARKER: &str = "[REDACTED]";
const SESSION_BEARER_MIN_LEN: usize = 8;
const MEMORY_BEARER_MIN_LEN: usize = 16;

const CREDENTIAL_ASSIGN_KEYS: [&str; 10] = [
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "api-key",
    "apikey",
    "access_key",
    "private_key",
    "secret_key",
];

const TOKEN_PREFIXES: [(&str, usize, bool); 12] = [
    ("sk-", 16, true),
    ("ghp_", 20, false),
    ("github_pat_", 20, true),
    ("glpat-", 20, true),
    ("xoxb-", 10, true),
    ("xoxa-", 10, true),
    ("xoxp-", 10, true),
    ("xoxr-", 10, true),
    ("xoxs-", 10, true),
    ("AKIA", 16, false),
    ("ASIA", 16, false),
    ("AIza", 35, true),
];

/// Redacts common credential patterns (API keys, bearer tokens, URL passwords,
/// secret assignments, JWTs, and PEM private keys) from arbitrary text.
pub fn redact_credentials_text(input: &str) -> String {
    redact_credentials_with_bearer_min(input, SESSION_BEARER_MIN_LEN)
}

/// Memory refuses bearer tokens of 16+ characters, so prose such as "use Bearer authentication" stays admissible.
pub(crate) fn contains_credential(text: &str) -> bool {
    redact_credentials_with_bearer_min(text, MEMORY_BEARER_MIN_LEN) != text
}

fn redact_credentials_with_bearer_min(input: &str, bearer_min: usize) -> String {
    if input.len() < MIN_SECRET_LEN {
        return input.to_string();
    }
    let mut out = redact_pem_blocks(input);
    out = redact_prefixed_tokens(&out);
    out = redact_bearer_tokens(&out, bearer_min);
    out = redact_url_passwords(&out);
    out = redact_key_assignments(&out);
    redact_jwts(&out)
}

fn is_word_boundary_before(text: &str, at: usize) -> bool {
    !text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn redact_pem_blocks(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let Some(start) = lower.find("-----begin ") else {
        return input.to_string();
    };
    let Some(header_end) = lower[start..].find("private key-----") else {
        return input.to_string();
    };
    let after_header = start + header_end + "private key-----".len();
    let end = lower[after_header..]
        .find("-----end ")
        .and_then(|rel| {
            let close_start = after_header + rel;
            lower[close_start..].find("-----").map(|_| {
                let tail = &lower[close_start + 5..];
                tail.find("-----").map_or(input.len(), |t| close_start + 5 + t + 5)
            })
        })
        .unwrap_or(after_header);
    let mut out = String::with_capacity(input.len());
    out.push_str(&input[..start]);
    out.push_str(REDACTED_MARKER);
    out.push_str(&redact_pem_blocks(&input[end..]));
    out
}

fn match_prefixed_token_end(input: &str, at: usize) -> Option<usize> {
    if !is_word_boundary_before(input, at) {
        return None;
    }
    let slice = &input[at..];
    for &(prefix, min_len, allow_dash) in &TOKEN_PREFIXES {
        if let Some(rest) = slice.strip_prefix(prefix) {
            let body_len: usize = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || (allow_dash && matches!(c, '-' | '_')))
                .map(char::len_utf8)
                .sum();
            if body_len >= min_len {
                return Some(at + prefix.len() + body_len);
            }
        }
    }
    None
}

fn redact_prefixed_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some(end) = match_prefixed_token_end(input, cursor) {
            out.push_str(REDACTED_MARKER);
            cursor = end;
        } else {
            let ch = input[cursor..].chars().next().unwrap_or_default();
            out.push(ch);
            cursor += ch.len_utf8();
        }
    }
    out
}

fn is_bearer_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '+' | '/' | '=')
}

fn redact_bearer_tokens(input: &str, min_len: usize) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find("bearer ") {
        let at = cursor + rel;
        let prefix_end = at + "bearer ".len();
        out.push_str(&input[cursor..prefix_end]);
        let token_len: usize = input[prefix_end..]
            .chars()
            .take_while(|&c| is_bearer_token_char(c))
            .map(char::len_utf8)
            .sum();
        if is_word_boundary_before(input, at) && token_len >= min_len {
            out.push_str(REDACTED_MARKER);
            cursor = prefix_end + token_len;
        } else {
            cursor = prefix_end;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn redact_url_passwords(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(rel) = input[cursor..].find("://") {
        let auth_start = cursor + rel + 3;
        let auth_len = input[auth_start..]
            .find(['/', ' ', '\t', '\n', '\r', '"', '\''])
            .unwrap_or(input.len() - auth_start);
        let authority = &input[auth_start..auth_start + auth_len];
        if let Some((userinfo, _)) = authority.split_once('@')
            && let Some((user, password)) = userinfo.split_once(':')
            && !password.is_empty()
        {
            let pass_start = auth_start + user.len() + 1;
            let pass_end = auth_start + userinfo.len();
            out.push_str(&input[cursor..pass_start]);
            out.push_str(REDACTED_MARKER);
            cursor = pass_end;
        } else {
            out.push_str(&input[cursor..auth_start]);
            cursor = auth_start;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn assignment_value_span(rest: &str) -> Option<(usize, usize)> {
    let after_ws = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let sep_slice = &rest[after_ws..];
    let is_equals = sep_slice.starts_with('=');
    if !is_equals && !sep_slice.starts_with(':') {
        return None;
    }
    let after_sep = &sep_slice[1..];
    let val_start = after_ws + 1 + (after_sep.len() - after_sep.trim_start_matches([' ', '\t']).len());
    let val_slice = &rest[val_start..];
    if let Some(quote) = val_slice.chars().next().filter(|&c| c == '"' || c == '\'') {
        let inner = &val_slice[1..];
        let end = inner.find(quote).map_or(val_slice.len(), |i| i + 2);
        return (end > 2).then_some((val_start, val_start + end));
    }
    let val_len: usize = val_slice
        .chars()
        .take_while(|c| !c.is_whitespace() && !matches!(c, ',' | ';' | '"' | '\'' | ')'))
        .map(char::len_utf8)
        .sum();
    (val_len > 0 || is_equals).then_some((val_start, val_start + val_len))
}

fn redact_key_assignments(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let next = CREDENTIAL_ASSIGN_KEYS
            .iter()
            .filter_map(|key| lower[cursor..].find(key).map(|rel| (cursor + rel, key.len())))
            .min_by_key(|&(pos, len)| (pos, std::cmp::Reverse(len)));
        let Some((at, key_len)) = next else {
            break;
        };
        let after_key = at + key_len;
        if is_word_boundary_before(input, at)
            && let Some((v_start, v_end)) = assignment_value_span(&input[after_key..])
        {
            out.push_str(&input[cursor..after_key + v_start]);
            out.push_str(REDACTED_MARKER);
            cursor = after_key + v_end;
        } else {
            out.push_str(&input[cursor..after_key]);
            cursor = after_key;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn is_jwt_token(word: &str) -> bool {
    let trimmed = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '_');
    let mut parts = trimmed.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    h.starts_with("eyJ")
        && [h, p, s].iter().all(|part| {
            part.len() >= 8
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
}

fn redact_jwts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() {
            let word = &input[cursor..idx];
            if is_jwt_token(word) {
                out.push_str(REDACTED_MARKER);
            } else {
                out.push_str(word);
            }
            out.push(ch);
            cursor = idx + ch.len_utf8();
        }
    }
    let tail = &input[cursor..];
    if is_jwt_token(tail) {
        out.push_str(REDACTED_MARKER);
    } else {
        out.push_str(tail);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_credentials_text_masks_common_credential_shapes() {
        assert_eq!(
            redact_credentials_text("use sk-ant-api03-abcdefghijklmnop123456 now"),
            "use [REDACTED] now"
        );
        assert_eq!(
            redact_credentials_text("ghp_abcdefghijklmnopqrstuvwxyz012345"),
            "[REDACTED]"
        );
        assert_eq!(
            redact_credentials_text("Authorization: Bearer abcdef0123456789abcdef"),
            "Authorization: Bearer [REDACTED]"
        );
        assert_eq!(
            redact_credentials_text("postgres://admin:supersecret@db.internal:5432/app"),
            "postgres://admin:[REDACTED]@db.internal:5432/app"
        );
        assert_eq!(
            redact_credentials_text("api_key=my_secret_123 and password: \"hunt3r2\""),
            "api_key=[REDACTED] and password: [REDACTED]"
        );
        assert_eq!(
            redact_credentials_text("jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3OCJ9.signature12345"),
            "jwt [REDACTED]"
        );
        assert_eq!(
            redact_credentials_text("plain PostgreSQL choice"),
            "plain PostgreSQL choice"
        );
    }

    #[test]
    fn short_sk_tokens_and_secret_key_assignments_are_redacted() {
        for (input, expected) in [
            ("key sk-abcdefghijklmnop end", "key [REDACTED] end"),
            ("key sk-abcdefghijklmnopqrs end", "key [REDACTED] end"),
            ("secret_key=hunter2hunter2", "secret_key=[REDACTED]"),
        ] {
            assert_eq!(redact_credentials_text(input), expected, "{input:?}");
        }
    }
}
