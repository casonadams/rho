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

    fn toggle_bold(&mut self, out: &mut String) {
        let bold_style = anstyle::Style::new().bold();
        if self.in_bold {
            out.push_str(&bold_style.render_reset().to_string());
            self.in_bold = false;
        } else {
            out.push_str(&bold_style.render().to_string());
            self.in_bold = true;
        }
    }

    fn toggle_italic(&mut self, out: &mut String) {
        let italic_style = anstyle::Style::new().italic();
        if self.in_italic {
            out.push_str(&italic_style.render_reset().to_string());
            self.in_italic = false;
        } else {
            out.push_str(&italic_style.render().to_string());
            self.in_italic = true;
        }
    }

    fn toggle_code(&mut self, out: &mut String, theme: &Theme) {
        if self.in_code {
            out.push_str(&theme.code_inline.render_reset().to_string());
            self.in_code = false;
        } else {
            out.push_str(&theme.code_inline.render().to_string());
            self.in_code = true;
        }
    }

    fn handle_pending_star(&mut self, first: char, out: &mut String) -> usize {
        self.pending_star = false;
        if first == '*' {
            self.toggle_bold(out);
            1
        } else if first.is_whitespace() {
            out.push('*');
            0
        } else {
            self.toggle_italic(out);
            0
        }
    }

    fn handle_star(&mut self, chars: &[char], i: usize, out: &mut String) -> usize {
        if i + 1 == chars.len() {
            self.pending_star = true;
            return 1;
        }
        if self.in_italic {
            if i > 0 && chars[i - 1].is_whitespace() {
                out.push('*');
            } else {
                self.toggle_italic(out);
            }
        } else if chars[i + 1].is_whitespace() {
            out.push('*');
        } else {
            self.toggle_italic(out);
        }
        1
    }

    fn process_token_char(&mut self, chars: &[char], i: usize, out: &mut String, theme: &Theme) -> usize {
        if chars[i] == '`' {
            self.toggle_code(out, theme);
            1
        } else if self.in_code {
            out.push(chars[i]);
            1
        } else if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            self.toggle_bold(out);
            2
        } else if chars[i] == '*' {
            self.handle_star(chars, i, out)
        } else {
            out.push(chars[i]);
            1
        }
    }

    pub fn render_inline_token(&mut self, token: &str, theme: &Theme) -> String {
        let mut out = String::new();
        let chars: Vec<char> = token.chars().collect();
        let len = chars.len();
        let mut i = 0;

        if self.pending_star && len > 0 {
            i = self.handle_pending_star(chars[0], &mut out);
        }

        while i < len {
            i += self.process_token_char(&chars, i, &mut out, theme);
        }
        out
    }
}
