//! Inline token streaming tracking for MarkdownRenderer.

use crate::ui::theme::Theme;

#[derive(Default)]
pub struct InlineStreamTracker {
    in_bold: bool,
    in_italic: bool,
    in_code: bool,
}

impl InlineStreamTracker {
    pub fn render_inline_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let bold_style = anstyle::Style::new().bold();
        let italic_style = anstyle::Style::new().italic();

        let chars: Vec<char> = token.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            if chars[i] == '`' {
                if self.in_code {
                    out.push_str(&theme.code_inline.render_reset().to_string());
                    self.in_code = false;
                } else {
                    out.push_str(&theme.code_inline.render().to_string());
                    self.in_code = true;
                }
                i += 1;
                continue;
            }

            if self.in_code {
                out.push(chars[i]);
                i += 1;
                continue;
            }

            if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
                if self.in_bold {
                    out.push_str(&bold_style.render_reset().to_string());
                    self.in_bold = false;
                } else {
                    out.push_str(&bold_style.render().to_string());
                    self.in_bold = true;
                }
                i += 2;
                continue;
            }

            if chars[i] == '*' && (i + 1 == len || chars[i + 1] != '*') {
                if self.in_italic {
                    out.push_str(&italic_style.render_reset().to_string());
                    self.in_italic = false;
                } else {
                    out.push_str(&italic_style.render().to_string());
                    self.in_italic = true;
                }
                i += 1;
                continue;
            }

            out.push(chars[i]);
            i += 1;
        }

        out
    }
}
