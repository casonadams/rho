//! Inline token streaming tracking for MarkdownRenderer.

use crate::ui::theme::Theme;

#[derive(Default)]
pub struct InlineStreamTracker {
    in_bold: bool,
    in_italic: bool,
    in_code: bool,
    pending_star: bool,
}

impl InlineStreamTracker {
    pub fn reset_line(&mut self) -> String {
        let mut out = String::new();
        if self.pending_star {
            out.push('*');
            self.pending_star = false;
        }
        if self.in_bold {
            out.push_str(&anstyle::Style::new().bold().render_reset().to_string());
            self.in_bold = false;
        }
        if self.in_italic {
            out.push_str(&anstyle::Style::new().italic().render_reset().to_string());
            self.in_italic = false;
        }
        if self.in_code {
            out.push_str(&anstyle::Style::new().render_reset().to_string());
            self.in_code = false;
        }
        out
    }

    pub fn render_inline_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let bold_style = anstyle::Style::new().bold();
        let italic_style = anstyle::Style::new().italic();

        let chars: Vec<char> = token.chars().collect();
        let len = chars.len();
        let mut i = 0;

        if self.pending_star && len > 0 {
            self.pending_star = false;
            if chars[0] == '*' {
                if self.in_bold {
                    out.push_str(&bold_style.render_reset().to_string());
                    self.in_bold = false;
                } else {
                    out.push_str(&bold_style.render().to_string());
                    self.in_bold = true;
                }
                i = 1;
            } else if chars[0].is_whitespace() {
                out.push('*');
            } else if self.in_italic {
                out.push_str(&italic_style.render_reset().to_string());
                self.in_italic = false;
            } else {
                out.push_str(&italic_style.render().to_string());
                self.in_italic = true;
            }
        }

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

            if chars[i] == '*' {
                if i + 1 == len {
                    self.pending_star = true;
                    i += 1;
                    continue;
                }
                if self.in_italic {
                    if i > 0 && chars[i - 1].is_whitespace() {
                        out.push('*');
                    } else {
                        out.push_str(&italic_style.render_reset().to_string());
                        self.in_italic = false;
                    }
                } else if chars[i + 1].is_whitespace() {
                    out.push('*');
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
