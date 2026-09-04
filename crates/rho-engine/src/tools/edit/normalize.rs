use std::borrow::Cow;

/// Detects whether the content predominantly uses CRLF or LF.
pub fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Normalizes newlines in `text` to match `target_ending`.
pub fn normalize_line_endings<'a>(text: &'a str, target_ending: &str) -> Cow<'a, str> {
    if target_ending == "\r\n" {
        if !text.contains('\n') || (text.contains("\r\n") && !text.replace("\r\n", "").contains('\n')) {
            Cow::Borrowed(text)
        } else {
            let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
            Cow::Owned(normalized)
        }
    } else if !text.contains("\r\n") {
        Cow::Borrowed(text)
    } else {
        let normalized = text.replace("\r\n", "\n");
        Cow::Owned(normalized)
    }
}

/// Checks if `content` contains a sequence of lines that matches `old_text`
/// when intra-line and leading/trailing whitespace are collapsed.
pub fn truncate_snippet(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

pub fn has_whitespace_relaxed_match(content: &str, old_text: &str) -> bool {
    let clean_old = collapse_whitespace(old_text);
    if clean_old.is_empty() {
        return false;
    }
    let clean_content = collapse_whitespace(content);
    clean_content.contains(&clean_old)
}

fn collapse_whitespace(s: &str) -> String {
    s.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_line_ending() {
        assert_eq!(detect_line_ending("hello\r\nworld\r\n"), "\r\n");
        assert_eq!(detect_line_ending("hello\nworld\n"), "\n");
        assert_eq!(detect_line_ending("single line"), "\n");
    }

    #[test]
    fn test_normalize_line_endings() {
        assert_eq!(normalize_line_endings("a\nb\n", "\r\n"), "a\r\nb\r\n");
        assert_eq!(normalize_line_endings("a\r\nb\r\n", "\n"), "a\nb\n");
        assert_eq!(normalize_line_endings("a\r\nb\r\n", "\r\n"), "a\r\nb\r\n");
        assert_eq!(normalize_line_endings("a\nb\n", "\n"), "a\nb\n");
    }

    #[test]
    fn test_whitespace_relaxed_match() {
        let content = "    fn run() {\n        let x = 1;\n    }\n";
        let target = "  fn run() {\n    let x = 1;\n  }";
        assert!(has_whitespace_relaxed_match(content, target));

        let different = "  fn other() {\n    let y = 2;\n  }";
        assert!(!has_whitespace_relaxed_match(content, different));
    }
}
