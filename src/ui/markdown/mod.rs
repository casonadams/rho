//! Markdown streaming renderer with terminal-friendly formatting.
//!
//! Submodules:
//! - [`renderer`]: the core `MarkdownRenderer` state machine that processes tokens line-by-line.
//! - [`highlight`]: syntect-backed code-block syntax highlighting.
//! - [`elements`]: inline-element and table rendering.
//!
//! Public API is re-exported here so external callers continue to use
//! `crate::ui::markdown::{MarkdownRenderer, render_inline_elements, ...}`.

mod elements;
mod highlight;
mod renderer;

#[cfg(test)]
mod tests;

pub use elements::{
    is_table_divider, is_table_line, render_inline_elements, render_markdown_table, strip_markdown_decorations,
};
pub use highlight::highlight_code_line;
pub use renderer::MarkdownRenderer;
