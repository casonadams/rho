use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone)]
pub struct StreamingSpinner<'a> {
    frame_index: usize,
    label: Option<&'a str>,
    style: Style,
}

impl<'a> StreamingSpinner<'a> {
    pub fn new(tick: usize) -> Self {
        Self {
            frame_index: tick % SPINNER_FRAMES.len(),
            label: None,
            style: Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        }
    }

    pub fn with_label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn current_frame(tick: usize) -> &'static str {
        SPINNER_FRAMES[tick % SPINNER_FRAMES.len()]
    }
}

impl Widget for StreamingSpinner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let frame_char = SPINNER_FRAMES[self.frame_index];
        let x = area.x;
        let y = area.y;
        buf.set_string(x, y, frame_char, self.style);
        if let Some(lbl) = self.label
            && area.width > 2
        {
            let lbl_style = Style::default().fg(Color::White);
            let max_len = (area.width - 2) as usize;
            let truncated: String = lbl.chars().take(max_len).collect();
            buf.set_string(x + 2, y, &truncated, lbl_style);
        }
    }
}
