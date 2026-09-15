use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Gauge, Widget};
use rho_ui_core::state::UpdateProgress;

#[derive(Debug, Clone)]
pub struct UpdateGauge<'a> {
    progress: &'a UpdateProgress,
}

impl<'a> UpdateGauge<'a> {
    pub fn new(progress: &'a UpdateProgress) -> Self {
        Self { progress }
    }
}

impl Widget for UpdateGauge<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 10 || area.height == 0 {
            return;
        }
        let ratio = (self.progress.percent / 100.0).clamp(0.0, 1.0);
        let label = if !self.progress.status.is_empty() {
            format!("{} ({:.0}%)", self.progress.status, self.progress.percent)
        } else {
            format!("{:.1}%", self.progress.percent)
        };
        let gauge = Gauge::default().ratio(ratio).label(label).gauge_style(
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        gauge.render(area, buf);
    }
}
