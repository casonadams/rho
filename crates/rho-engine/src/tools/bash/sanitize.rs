use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

static ANSI_ESCAPE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\].*?(?:\x07|\x1b\\)|[@-Z\\-_])").expect("valid ANSI regex")
});

pub fn strip_ansi(text: &str) -> Cow<'_, str> {
    if !text.contains('\x1b') {
        return Cow::Borrowed(text);
    }
    ANSI_ESCAPE_REGEX.replace_all(text, "")
}

#[inline]
fn is_filtered_char(c: char) -> bool {
    let u = c as u32;
    if u == 0x09 || u == 0x0A || u == 0x0D {
        return false;
    }
    if u <= 0x1F {
        return true;
    }
    (0xFFF9..=0xFFFB).contains(&u)
}

/// Sanitizes binary output to remove ANSI color escapes, control characters,
/// and Unicode format characters that corrupt model context and terminal rendering.
pub fn sanitize_binary_output(text: &str) -> Cow<'_, str> {
    let stripped = strip_ansi(text);
    if !stripped.chars().any(is_filtered_char) {
        return stripped;
    }
    let mut out = String::with_capacity(stripped.len());
    for c in stripped.chars() {
        if !is_filtered_char(c) {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

pub fn split_at_incomplete_ansi(text: &str) -> (&str, &str) {
    let Some(esc_idx) = text.rfind('\x1b') else {
        return (text, "");
    };
    let tail = &text[esc_idx..];
    if tail.len() > 64 || is_complete_escape_tail(tail) {
        (text, "")
    } else {
        (&text[..esc_idx], tail)
    }
}

fn is_complete_escape_tail(tail: &str) -> bool {
    let bytes = tail.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    match bytes[1] {
        b'[' => bytes[2..].iter().any(|&b| (0x40..=0x7E).contains(&b)),
        b']' => tail[2..].contains('\x07') || tail[2..].contains("\x1b\\"),
        b'@'..=b'_' => true,
        _ => bytes.len() >= 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_preserves_clean_text_and_standard_whitespace() {
        let input = "hello\tworld\r\nthis is clean!";
        assert_eq!(sanitize_binary_output(input), input);
    }

    #[test]
    fn test_sanitize_removes_binary_control_chars_and_format_chars() {
        let input = "hello\x00\x07\x1bworld\u{fff9}foo\u{fffb}";
        assert_eq!(sanitize_binary_output(input), "helloworldfoo");
    }

    #[test]
    fn test_sanitize_strips_ansi_color_escapes() {
        let input =
            "\x1b[32mFinished\x1b[0m \x1b[1mdev\x1b[0m profile [\x1b[33munoptimized\x1b[0m + \x1b[36mdebuginfo\x1b[0m]";
        assert_eq!(
            sanitize_binary_output(input),
            "Finished dev profile [unoptimized + debuginfo]"
        );
    }

    #[test]
    fn test_split_at_incomplete_ansi_handles_split_csi_sequence() {
        assert_eq!(split_at_incomplete_ansi("hello \x1b[4"), ("hello ", "\x1b[4"));
        assert_eq!(split_at_incomplete_ansi("hello \x1b"), ("hello ", "\x1b"));
        assert_eq!(split_at_incomplete_ansi("hello \x1b[40m"), ("hello \x1b[40m", ""));
        assert_eq!(
            split_at_incomplete_ansi("hello \x1b[40mworld"),
            ("hello \x1b[40mworld", "")
        );
        assert_eq!(split_at_incomplete_ansi("plain text"), ("plain text", ""));
    }
}
