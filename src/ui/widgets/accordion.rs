use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

#[derive(Debug, Clone)]
pub struct ThinkingAccordion<'a> {
    token_count: u64,
    duration_secs: f64,
    preview_lines: Vec<&'a str>,
    is_expanded: bool,
}

impl<'a> ThinkingAccordion<'a> {
    pub fn new(token_count: u64, duration_secs: f64, is_expanded: bool) -> Self {
        Self {
            token_count,
            duration_secs,
            preview_lines: Vec::new(),
            is_expanded,
        }
    }

    pub fn with_preview(mut self, lines: Vec<&'a str>) -> Self {
        self.preview_lines = lines;
        self
    }
}

impl Widget for ThinkingAccordion<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 15 || area.height == 0 {
            return;
        }
        let header_icon = if self.is_expanded { "▾" } else { "▸" };
        let tokens_str = rho_ui_core::state::format_tokens(self.token_count);
        let header = format!(
            "{header_icon} Thinking ({tokens_str} tokens · {:.1}s)",
            self.duration_secs
        );
        let header_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::DIM);

        if !self.is_expanded || area.height < 2 {
            buf.set_string(area.x, area.y, &header, header_style);
            return;
        }

        buf.set_string(area.x, area.y, &header, header_style);
        let content_style = Style::default().fg(Color::DarkGray);
        for (i, line) in self.preview_lines.iter().enumerate() {
            let row = area.y + 1 + (i as u16);
            if row >= area.y + area.height {
                break;
            }
            let max_w = area.width.saturating_sub(2) as usize;
            let display_line: String = line.chars().take(max_w).collect();
            buf.set_string(area.x + 2, row, &display_line, content_style);
        }
    }
}
