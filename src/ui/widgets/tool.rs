use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Widget};
use rho_ui_core::state::ActiveToolState;

use super::spinner::StreamingSpinner;

#[derive(Debug, Clone)]
pub struct ActiveToolCard<'a> {
    state: &'a ActiveToolState,
    tick: usize,
}

impl<'a> ActiveToolCard<'a> {
    pub fn new(state: &'a ActiveToolState, tick: usize) -> Self {
        Self { state, tick }
    }
}

impl Widget for ActiveToolCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height < 3 {
            return;
        }
        let border_color = if self.state.is_running {
            Color::Cyan
        } else if self.state.is_denied {
            Color::Yellow
        } else if self.state.error.is_some() {
            Color::Red
        } else {
            Color::Green
        };

        let title = format!(" Tool: {} ", self.state.name);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(title);
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let spinner_char = if self.state.is_running {
            StreamingSpinner::current_frame(self.tick)
        } else if self.state.is_denied {
            "⊘"
        } else if self.state.error.is_some() {
            "✗"
        } else {
            "✓"
        };

        let status_text = if self.state.is_running {
            "RUNNING"
        } else if self.state.is_denied {
            "DENIED"
        } else if self.state.error.is_some() {
            "ERROR"
        } else {
            "DONE"
        };

        let duration_str = if self.state.elapsed_ms >= 1000 {
            format!("{:.1}s", self.state.elapsed_ms as f64 / 1000.0)
        } else {
            format!("{}ms", self.state.elapsed_ms)
        };

        let header = format!("{spinner_char} [{status_text}] {duration_str}");
        buf.set_string(
            inner.x,
            inner.y,
            &header,
            Style::default().fg(border_color).add_modifier(Modifier::BOLD),
        );

        if inner.height > 1 && !self.state.arguments_summary.is_empty() {
            let max_w = inner.width as usize;
            let summary: String = self.state.arguments_summary.chars().take(max_w).collect();
            buf.set_string(inner.x, inner.y + 1, &summary, Style::default().fg(Color::DarkGray));
        }

        if inner.height > 2
            && let Some(err) = &self.state.error
        {
            let max_w = inner.width as usize;
            let err_summary: String = err.chars().take(max_w).collect();
            buf.set_string(inner.x, inner.y + 2, &err_summary, Style::default().fg(Color::Red));
        }
    }
}
