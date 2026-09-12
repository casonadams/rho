//! Mermaid diagram rendering via headless Merman ASCII/Unicode engine.

use merman::OperationControl;
use merman::ascii::{AsciiLayoutProfile, AsciiRenderOptions, AsciiViewportPolicy, OverflowPolicy};
use merman::render::{AsciiRequest, RenderOutput, RenderRequest, Renderer};

/// Attempts to parse and render a Mermaid diagram to Unicode box-drawing text.
///
/// Returns `Some(rendered)` if parsing and rendering succeed.
/// Returns `None` if the source is invalid Mermaid syntax or cannot be rendered.
pub fn render_diagram(source: &str, width: usize) -> Option<String> {
    let renderer = Renderer::new();
    let default_options = AsciiRenderOptions::unicode();

    let primary = render_with_options(&renderer, source, width, default_options);

    if width > 0
        && let Some(primary_text) = &primary
    {
        let max_w = primary_text
            .lines()
            .map(crate::ui::interactive::footer::visible_width)
            .max()
            .unwrap_or(0);

        if max_w > width {
            let compact_options = default_options.with_layout_profile(AsciiLayoutProfile::Compact);
            if let Some(compact_text) = render_with_options(&renderer, source, width, compact_options) {
                let compact_max_w = compact_text
                    .lines()
                    .map(crate::ui::interactive::footer::visible_width)
                    .max()
                    .unwrap_or(0);

                if compact_max_w < max_w {
                    return Some(compact_text);
                }
            }
        }
    }

    primary
}

fn render_with_options(renderer: &Renderer, source: &str, width: usize, options: AsciiRenderOptions) -> Option<String> {
    let viewport = if width > 0 {
        AsciiViewportPolicy::with_max_width(width).overflow(OverflowPolicy::Allow)
    } else {
        AsciiViewportPolicy::unrestricted()
    };

    let ascii_request = AsciiRequest {
        options,
        viewport,
        resources: merman::ascii::AsciiResourcePolicy::default(),
    };

    let request = RenderRequest::ascii(source, OperationControl::new(), ascii_request);

    match renderer.render(request) {
        Ok(RenderOutput::Ascii(Some(output))) => {
            let text = output.text;
            if text.trim().is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}
