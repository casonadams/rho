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

/// Discards a result that has no recovery path at its call site, so the discard is explicit.
/// A `UiPortError::Closed` means the interactive controller has exited; a failed stdout write
/// means the reader is gone; a failed kill or reap means the child already exited.
pub(crate) fn best_effort<T, E>(result: Result<T, E>) {
    let _ = result;
}
