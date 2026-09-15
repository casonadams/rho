//! Terminal UI: streaming markdown rendering, the interactive controller
//! (transcript caching, redraw batching, keymaps), block painting, and themes.

pub mod block;
pub mod interactive;
pub mod markdown;
pub mod render;
pub mod stream;
pub mod theme;

pub use interactive::cache;
pub use markdown::MarkdownRenderer;
pub use render::TerminalRenderer;
pub use stream::ToolStreamPort;
pub use theme::Theme;

pub fn terminal_width() -> u16 {
    crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80)
}
