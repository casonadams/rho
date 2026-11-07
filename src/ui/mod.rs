//! Terminal UI: streaming markdown rendering, the interactive controller
//! (transcript caching, redraw batching), block painting, and themes.

pub mod block;
pub mod interactive;
pub mod markdown;
pub mod render;
pub mod stream;
pub mod theme;
pub(crate) mod wrap;

pub use interactive::cache;
pub use markdown::MarkdownRenderer;
pub use render::TerminalRenderer;
pub use stream::ToolStreamPort;
pub use theme::Theme;
