//! Terminal UI: streaming markdown rendering, the interactive controller
//! (transcript caching, redraw batching, keymaps), block painting, and themes.

pub mod block;
pub mod editor;
pub mod interactive;
pub mod markdown;
pub mod modal;
pub mod render;
pub mod terminal;
pub mod theme;
pub mod widgets;

pub use editor::{EditorMode, TextAreaEditor, Vim, VimMode};
pub use markdown::MarkdownRenderer;
pub use modal::{
    AutocompletePopupView, PermissionPromptView, RemotePairModalView, StandardModalView, centered_modal_area,
    prompt_session_picker, render_modal, run_modal_view,
};
pub use render::TerminalRenderer;
pub use terminal::{
    MOUSE_SCROLL_VELOCITY, OSC133_ZONE_END, OSC133_ZONE_FINAL, OSC133_ZONE_START, RESIZE_DEBOUNCE_MILLIS,
    TERMINAL_BELL, TerminalGuard, TerminalRunner, install_terminal_panic_hook, write_turn_completion,
};
pub use theme::Theme;
pub use widgets::{ActiveToolCard, CompactionBadge, StreamingSpinner, ThinkingAccordion, UpdateGauge};

pub trait TerminalSurface {
    fn size(&self) -> std::io::Result<ratatui::layout::Rect>;
    fn draw<F>(&mut self, f: F) -> std::io::Result<ratatui::CompletedFrame<'_>>
    where
        F: FnOnce(&mut ratatui::Frame);
    fn clear(&mut self) -> std::io::Result<()>;
    fn hide_cursor(&mut self) -> std::io::Result<()>;
    fn show_cursor(&mut self) -> std::io::Result<()>;
    fn set_cursor_position<P: Into<ratatui::layout::Position>>(&mut self, position: P) -> std::io::Result<()>;
}

impl<B: ratatui::backend::Backend> TerminalSurface for ratatui::Terminal<B> {
    fn size(&self) -> std::io::Result<ratatui::layout::Rect> {
        let size = self.size().map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(ratatui::layout::Rect::new(0, 0, size.width, size.height))
    }

    fn draw<F>(&mut self, f: F) -> std::io::Result<ratatui::CompletedFrame<'_>>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.draw(f).map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.clear().map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        self.hide_cursor().map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        self.show_cursor().map_err(|e| std::io::Error::other(e.to_string()))
    }

    fn set_cursor_position<P: Into<ratatui::layout::Position>>(&mut self, position: P) -> std::io::Result<()> {
        self.set_cursor_position(position)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

pub trait TerminalComponent {
    fn render(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect);
    fn desired_height(&self, _width: u16) -> u16 {
        0
    }
}

pub trait PromptEditor {
    fn text(&self) -> String;
    fn set_text(&mut self, text: &str);
    fn is_empty(&self) -> bool;
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool;
    fn insert_str(&mut self, text: &str);
    fn clear(&mut self);
    fn cursor(&self) -> (usize, usize);
}

pub trait ModalView {
    fn title(&self) -> &str;
    fn selected_index(&self) -> usize;
    fn item_count(&self) -> usize;
    fn filter(&self) -> &str;
    fn set_filter(&mut self, query: &str);
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool;
    fn render(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect);
    fn is_open(&self) -> bool {
        true
    }
    /// False while the view still needs input after Enter, so the modal driver
    /// keeps the loop alive instead of closing on the first Enter.
    fn is_submitted(&self) -> bool {
        true
    }
}

pub fn terminal_width() -> u16 {
    crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80)
}

pub fn terminal_height() -> u16 {
    crossterm::terminal::size().map(|(_, h)| h).unwrap_or(24)
}
