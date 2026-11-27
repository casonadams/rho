//! Terminal UI: streaming markdown rendering, the interactive controller
//! (transcript caching, redraw batching), block painting, and themes.

pub mod block;
pub(crate) mod fuzzy;
pub mod interactive;
pub mod markdown;
pub mod render;
pub(crate) mod terminal_stream;
pub mod theme;
pub(crate) mod wrap;

pub use interactive::cache;
pub use markdown::MarkdownRenderer;
pub use render::TerminalRenderer;
pub use theme::Theme;
