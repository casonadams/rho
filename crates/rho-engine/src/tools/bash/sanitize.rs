use std::borrow::Cow;

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

/// Sanitizes binary output to remove control characters and Unicode format characters
/// that can crash or corrupt terminal rendering and string width calculations.
pub fn sanitize_binary_output(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_filtered_char) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if !is_filtered_char(c) {
            out.push(c);
        }
    }
    Cow::Owned(out)
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
}
