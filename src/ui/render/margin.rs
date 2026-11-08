//! One-column left margin shared by streamed and committed agent output.

use anstyle::Style;

pub(crate) fn margin_width(width: usize) -> usize {
    if width == 0 { 0 } else { width.saturating_sub(1).max(1) }
}

/// Indents every non-empty line by one space and optionally re-applies a style
/// after each reset so inline markdown cannot drop it mid-line.
#[derive(Debug)]
pub(crate) struct Margin {
    style: Option<Style>,
    at_line_start: bool,
}

impl Margin {
    pub(crate) fn new(style: Option<Style>) -> Self {
        Self {
            style,
            at_line_start: true,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.at_line_start = true;
    }

    pub(crate) fn apply(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 8);
        for (idx, segment) in text.split('\n').enumerate() {
            if idx > 0 {
                out.push('\n');
                self.at_line_start = true;
            }
            if segment.is_empty() {
                continue;
            }
            if self.at_line_start {
                out.push(' ');
                self.at_line_start = false;
            }
            match self.style {
                Some(style) => {
                    let restyled = segment.replace("\x1b[0m", &format!("\x1b[0m{style}"));
                    out.push_str(&format!("{style}{restyled}{style:#}"));
                }
                None => out.push_str(segment),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_each_non_empty_line_once_across_chunks() {
        let mut margin = Margin::new(None);
        let out = format!(
            "{}{}{}",
            margin.apply("ab"),
            margin.apply("c\n\nd"),
            margin.apply("e\n")
        );
        assert_eq!(out, " abc\n\n de\n");
    }

    #[test]
    fn reapplies_style_after_resets() {
        let dim = Style::new().dimmed();
        let mut margin = Margin::new(Some(dim));
        let out = margin.apply("a\x1b[0m b");
        assert_eq!(out, format!(" {dim}a\x1b[0m{dim} b{dim:#}"));
    }

    #[test]
    fn reset_restores_line_start() {
        let mut margin = Margin::new(None);
        margin.apply("x");
        margin.reset();
        assert_eq!(margin.apply("y"), " y");
    }

    #[test]
    fn margin_width_reserves_one_column() {
        assert_eq!((margin_width(0), margin_width(1), margin_width(80)), (0, 1, 79));
    }
}
