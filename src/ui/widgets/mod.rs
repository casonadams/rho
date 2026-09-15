pub mod accordion;
pub mod compaction;
pub mod gauge;
pub mod spinner;
pub mod tool;

pub use accordion::ThinkingAccordion;
pub use compaction::CompactionBadge;
pub use gauge::UpdateGauge;
pub use spinner::{SPINNER_FRAMES, StreamingSpinner};
pub use tool::ActiveToolCard;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use rho_ui_core::state::{ActiveToolState, CompactionMilestone, UpdateProgress};

    fn buffer_text(backend: &TestBackend) -> String {
        let mut s = String::new();
        let buf = backend.buffer();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn test_active_tool_card_rendering() {
        let mut tool_state = ActiveToolState::running("bash", "call_1", "cargo check --workspace");
        tool_state.elapsed_ms = 1500;

        let card = ActiveToolCard::new(&tool_state, 3);
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(card, Rect::new(0, 0, 80, 5));
            })
            .unwrap();

        let text = buffer_text(terminal.backend());
        assert!(text.contains("Tool: bash"));
        assert!(text.contains("[RUNNING]"));
        assert!(text.contains("1.5s"));
        assert!(text.contains("cargo check --workspace"));

        tool_state.denied(Some("User rejected command".to_string()));
        let card_denied = ActiveToolCard::new(&tool_state, 0);
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(card_denied, Rect::new(0, 0, 80, 5));
            })
            .unwrap();

        let text_denied = buffer_text(terminal.backend());
        assert!(text_denied.contains("[DENIED]"));
        assert!(text_denied.contains("User rejected command"));
    }

    #[test]
    fn test_streaming_spinner_frames() {
        assert_eq!(StreamingSpinner::current_frame(0), "⠋");
        assert_eq!(StreamingSpinner::current_frame(1), "⠙");
        assert_eq!(StreamingSpinner::current_frame(10), "⠋");

        let spinner = StreamingSpinner::new(2).with_label("Working...");
        let backend = TestBackend::new(30, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(spinner, Rect::new(0, 0, 30, 1));
            })
            .unwrap();

        let text = buffer_text(terminal.backend());
        assert!(text.contains("⠹"));
        assert!(text.contains("Working..."));
    }

    #[test]
    fn test_update_gauge_rendering() {
        let progress = UpdateProgress {
            percent: 75.0,
            downloaded_bytes: 7500,
            total_bytes: Some(10000),
            status: "Downloading update".to_string(),
        };
        let gauge = UpdateGauge::new(&progress);
        let backend = TestBackend::new(50, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(gauge, Rect::new(0, 0, 50, 1));
            })
            .unwrap();

        let text = buffer_text(terminal.backend());
        assert!(text.contains("Downloading update (75%)"));
    }

    #[test]
    fn test_compaction_badge_rendering() {
        let milestone = CompactionMilestone::new(50_000, 10_000, 1200);
        assert_eq!(milestone.reduction_percent(), 80.0);

        let badge = CompactionBadge::new(&milestone);
        let backend = TestBackend::new(60, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(badge, Rect::new(0, 0, 60, 1));
            })
            .unwrap();

        let text = buffer_text(terminal.backend());
        assert!(text.contains("compacted: 50.0k → 10.0k (-80%) in 1.2s"));
    }

    #[test]
    fn test_thinking_accordion_collapsed_and_expanded() {
        let accordion = ThinkingAccordion::new(3500, 2.4, false);
        let backend = TestBackend::new(60, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(accordion, Rect::new(0, 0, 60, 4));
            })
            .unwrap();

        let text = buffer_text(terminal.backend());
        assert!(text.contains("▸ Thinking (3.5k tokens · 2.4s)"));

        let preview = vec!["Step 1: Inspecting code...", "Step 2: Designing solution..."];
        let accordion_expanded = ThinkingAccordion::new(3500, 2.4, true).with_preview(preview);
        let backend = TestBackend::new(60, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(accordion_expanded, Rect::new(0, 0, 60, 4));
            })
            .unwrap();

        let text_expanded = buffer_text(terminal.backend());
        assert!(text_expanded.contains("▾ Thinking"));
        assert!(text_expanded.contains("Step 1: Inspecting code..."));
        assert!(text_expanded.contains("Step 2: Designing solution..."));
    }
}
