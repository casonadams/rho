use crate::ui::markdown::stream::ChunkWordWrapper;
use crate::ui::theme::Theme;

pub struct ThinkingStreamTracker {
    wrapper: ChunkWordWrapper,
}

impl Default for ThinkingStreamTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ThinkingStreamTracker {
    pub fn new() -> Self {
        Self {
            wrapper: ChunkWordWrapper::new_thinking(),
        }
    }

    pub fn process_token(&mut self, token: &str, width: usize, theme: &Theme) -> String {
        let max_width = if width > 0 { width.saturating_sub(1).max(10) } else { 79 };
        self.wrapper.set_width(max_width);
        self.wrapper.set_style(Some(theme.dimmed));
        self.wrapper.process_chunk(token)
    }

    pub fn flush(&mut self, theme: &Theme) -> String {
        self.wrapper.set_style(Some(theme.dimmed));
        self.wrapper.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_thinking_wraps_on_word_boundaries() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("alpha beta ", 16, &theme));
        output.push_str(&tracker.process_token("gamma delta ", 16, &theme));
        output.push_str(&tracker.process_token("epsilon", 16, &theme));
        output.push_str(&tracker.flush(&theme));

        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 2);
        assert!(lines[0].contains("alpha"));
        assert!(lines[0].contains("beta"));
        assert!(lines[1].contains("gamma"));
        assert!(lines[1].contains("delta"));
    }

    #[test]
    fn stream_thinking_preserves_single_space_indent_on_wrapped_lines() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("first second third ", 12, &theme));
        output.push_str(&tracker.process_token("fourth", 12, &theme));
        output.push_str(&tracker.flush(&theme));

        for line in output.lines() {
            assert!(
                line.starts_with(' ') || line.starts_with("\x1b"),
                "each line must be indented with space: {line:?}"
            );
        }
    }

    #[test]
    fn stream_thinking_preserves_natural_line_breaks() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("Step 1: analyze\nStep 2: implement\n", 80, &theme));
        output.push_str(&tracker.flush(&theme));

        let clean = crate::ui::block::ANSI_PATTERN.replace_all(&output, "");
        let lines: Vec<&str> = clean.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], " Step 1: analyze");
        assert_eq!(lines[1], " Step 2: implement");
    }

    #[test]
    fn stream_thinking_preserves_natural_paragraph_breaks() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("Paragraph 1.\n\nParagraph 2.\n", 80, &theme));
        output.push_str(&tracker.flush(&theme));

        let clean = crate::ui::block::ANSI_PATTERN.replace_all(&output, "");
        let lines: Vec<&str> = clean.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], " Paragraph 1.");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], " Paragraph 2.");
    }

    #[test]
    fn stream_thinking_preserves_token_split_natural_line_breaks() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("Step 1: analyze", 80, &theme));
        output.push_str(&tracker.process_token("\n", 80, &theme));
        output.push_str(&tracker.process_token("Step 2: implement", 80, &theme));
        output.push_str(&tracker.flush(&theme));

        let clean = crate::ui::block::ANSI_PATTERN.replace_all(&output, "");
        let lines: Vec<&str> = clean.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], " Step 1: analyze");
        assert_eq!(lines[1], " Step 2: implement");
    }

    #[test]
    fn stream_thinking_handles_crlf_line_breaks() {
        let theme = Theme::default();
        let mut tracker = ThinkingStreamTracker::new();

        let mut output = String::new();
        output.push_str(&tracker.process_token("Line 1\r\nLine 2\r\n", 80, &theme));
        output.push_str(&tracker.flush(&theme));

        let clean = crate::ui::block::ANSI_PATTERN.replace_all(&output, "");
        assert!(!clean.contains('\r'), "carriage return should be stripped");
        let lines: Vec<&str> = clean.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], " Line 1");
        assert_eq!(lines[1], " Line 2");
    }
}
