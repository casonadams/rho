//! Human-readable text formatting shared across rho crates.

/// Formats a byte count as `512B`, `1.0KB`, or `1.0MB`.
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// The first `max_chars` characters of `text`, and whether anything was cut off after them.
pub fn truncate_chars(text: &str, max_chars: usize) -> (&str, bool) {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => (&text[..idx], true),
        None => (text, false),
    }
}

/// The first `max_chars` characters of `text`, and how many characters were cut off after them.
pub fn clip_chars(text: &str, max_chars: usize) -> (&str, usize) {
    let (kept, _) = truncate_chars(text, max_chars);
    (kept, text[kept.len()..].chars().count())
}

/// Trims a value and strips surrounding `"` and `'` quote characters.
pub fn unquote_scalar(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("", 0, ("", 0); "empty at zero")]
    #[test_case("", 3, ("", 0); "empty under limit")]
    #[test_case("abc", 3, ("abc", 0); "exactly at limit")]
    #[test_case("abcd", 3, ("abc", 1); "one over limit")]
    #[test_case("abcd", 0, ("", 4); "zero limit")]
    #[test_case("h\u{e9}llo", 2, ("h\u{e9}", 3); "two byte character")]
    #[test_case("\u{65e5}\u{672c}\u{8a9e}", 1, ("\u{65e5}", 2); "three byte characters")]
    fn clip_chars_reports_how_many_characters_were_omitted(text: &str, max: usize, expected: (&str, usize)) {
        assert_eq!(clip_chars(text, max), expected);
    }

    #[test]
    fn truncate_chars_cuts_on_character_boundaries() {
        assert_eq!(truncate_chars("abc", 3), ("abc", false));
        assert_eq!(truncate_chars("abcd", 3), ("abc", true));
        assert_eq!(truncate_chars("", 0), ("", false));
        assert_eq!(truncate_chars("h\u{e9}llo", 2), ("h\u{e9}", true));
        assert_eq!(truncate_chars("\u{65e5}\u{672c}\u{8a9e}", 1), ("\u{65e5}", true));
    }

    #[test_case("  \"name\" ", "name"; "padded double quoted")]
    #[test_case("'x'", "x"; "single quoted")]
    #[test_case("plain", "plain"; "unquoted")]
    #[test_case("", ""; "empty")]
    fn unquote_scalar_trims_and_strips_quote_characters(raw: &str, expected: &str) {
        assert_eq!(unquote_scalar(raw), expected);
    }

    #[test_case(0, "0B"; "zero bytes")]
    #[test_case(512, "512B"; "half kilobyte")]
    #[test_case(1023, "1023B"; "just under a kilobyte")]
    #[test_case(1024, "1.0KB"; "one kilobyte")]
    #[test_case(1536, "1.5KB"; "fractional kilobytes")]
    #[test_case(1024 * 1024, "1.0MB"; "one megabyte")]
    #[test_case(5 * 1024 * 1024 + 512 * 1024, "5.5MB"; "fractional megabytes")]
    fn format_size_uses_the_engine_truncation_units(bytes: usize, expected: &str) {
        assert_eq!(format_size(bytes), expected);
    }
}
