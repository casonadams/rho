//! Terminal-first flowchart rendering engine.

mod ast;
mod canvas;
mod layout;

#[cfg(test)]
mod tests;

pub use ast::Flowchart;
pub use layout::FlowchartLayout;

/// Attempts to parse and render a Mermaid flowchart (`graph` or `flowchart`) to a
/// compact terminal diagram.
///
/// Returns `Some(rendered)` if parsing succeeds.
/// Returns `None` if the diagram is not a flowchart or cannot be parsed.
pub fn render_flowchart(source: &str) -> Option<String> {
    let flowchart = Flowchart::parse(source)?;
    let rendered = FlowchartLayout::new(&flowchart).render();

    if rendered.trim().is_empty() {
        None
    } else {
        Some(rendered)
    }
}
