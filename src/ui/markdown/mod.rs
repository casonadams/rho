//! Markdown rendering engine with streaming support.
//!
//! Submodules:
//! - [`renderer`]: the core `MarkdownRenderer` state machine that processes tokens line-by-line.
//! - [`highlight`]: syntect-backed code-block syntax highlighting.
//! - [`diagram`]: mermaid diagram rendering and block tracking.
//! - [`elements`]: inline-element rendering (pulldown-cmark).
//! - [`table`]: markdown table parsing and layout.
//! - [`line`]: line-level element rendering and prefix buffering.
//! - [`stream`]: inline token streaming state tracker.

pub(crate) mod diagram;
mod elements;
mod highlight;
mod line;
mod renderer;
mod stream;
mod table;

#[cfg(test)]
mod tests;

pub use diagram::render_mermaid_block;
pub use elements::render_inline_elements;
pub use highlight::{CodeHighlighter, highlight_code_line};
pub use renderer::MarkdownRenderer;
pub use table::{is_table_divider, is_table_line, render_markdown_table, strip_markdown_decorations};
