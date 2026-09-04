//! Markdown rendering engine with streaming support.
//!
//! Submodules:
//! - [`renderer`]: the core `MarkdownRenderer` state machine that processes tokens line-by-line.
//! - [`highlight`]: syntect-backed code-block syntax highlighting.
//! - [`elements`]: inline-element and mermaid diagram rendering.
//! - [`table`]: markdown table parsing and layout.
//! - [`line`]: line-level element rendering and prefix buffering.
//! - [`stream`]: inline token streaming state tracker.
//! - [`spacing`]: block separation and newline normalization.
//! - [`mermaid`]: mermaid diagram block tracker.
//!
//! Public API is re-exported here so external callers continue to use
//! `crate::ui::markdown::{MarkdownRenderer, render_inline_elements, ...}`.

mod elements;
mod highlight;
mod line;
mod mermaid;
mod renderer;
mod spacing;
mod stream;
mod table;

#[cfg(test)]
mod tests;

pub use elements::{render_inline_elements, render_mermaid_block};
pub use highlight::highlight_code_line;
pub use renderer::MarkdownRenderer;
pub use table::{is_table_divider, is_table_line, render_markdown_table, strip_markdown_decorations};
