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

/// Trims a value and strips surrounding `"` and `'` quote characters.
pub fn unquote_scalar(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_cuts_on_character_boundaries() {
        assert_eq!(truncate_chars("abc", 3), ("abc", false));
        assert_eq!(truncate_chars("abcd", 3), ("abc", true));
        assert_eq!(truncate_chars("", 0), ("", false));
        assert_eq!(truncate_chars("h\u{e9}llo", 2), ("h\u{e9}", true));
        assert_eq!(truncate_chars("\u{65e5}\u{672c}\u{8a9e}", 1), ("\u{65e5}", true));
    }

    #[test]
    fn unquote_scalar_trims_and_strips_quote_characters() {
        for (raw, expected) in [("  \"name\" ", "name"), ("'x'", "x"), ("plain", "plain"), ("", "")] {
            assert_eq!(unquote_scalar(raw), expected, "{raw:?}");
        }
    }

    #[test]
    fn format_size_uses_the_engine_truncation_units() {
        for (bytes, expected) in [
            (0, "0B"),
            (512, "512B"),
            (1023, "1023B"),
            (1024, "1.0KB"),
            (1536, "1.5KB"),
            (1024 * 1024, "1.0MB"),
            (5 * 1024 * 1024 + 512 * 1024, "5.5MB"),
        ] {
            assert_eq!(format_size(bytes), expected, "{bytes}");
        }
    }
}
