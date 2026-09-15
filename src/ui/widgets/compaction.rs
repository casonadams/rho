use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use rho_ui_core::state::CompactionMilestone;

#[derive(Debug, Clone)]
pub struct CompactionBadge<'a> {
    milestone: &'a CompactionMilestone,
}

impl<'a> CompactionBadge<'a> {
    pub fn new(milestone: &'a CompactionMilestone) -> Self {
        Self { milestone }
    }
}

impl Widget for CompactionBadge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 15 || area.height == 0 {
            return;
        }
        let badge_text = format!("⚡ {}", self.milestone.format_badge());
        let max_w = area.width as usize;
        let text: String = badge_text.chars().take(max_w).collect();
        let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
        buf.set_string(area.x, area.y, &text, style);
    }
}
