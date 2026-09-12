//! Mermaid diagram rendering via headless Merman ASCII/Unicode engine.

use merman::OperationControl;
use merman::ascii::{AsciiColorMode, AsciiRenderOptions, AsciiViewportPolicy, OverflowPolicy};
use merman::render::{AsciiRequest, RenderOutput, RenderRequest, Renderer};

struct Preset {
    padding_x: usize,
    padding_y: usize,
    box_border_padding: usize,
    participant_spacing: usize,
    message_spacing: usize,
    wrap_width: usize,
}

const PRESETS: [Preset; 4] = [
    Preset {
        padding_x: 5,
        padding_y: 2,
        box_border_padding: 1,
        participant_spacing: 5,
        message_spacing: 1,
        wrap_width: 32,
    },
    Preset {
        padding_x: 3,
        padding_y: 2,
        box_border_padding: 1,
        participant_spacing: 3,
        message_spacing: 1,
        wrap_width: 28,
    },
    Preset {
        padding_x: 2,
        padding_y: 2,
        box_border_padding: 1,
        participant_spacing: 2,
        message_spacing: 1,
        wrap_width: 24,
    },
    Preset {
        padding_x: 1,
        padding_y: 1,
        box_border_padding: 0,
        participant_spacing: 1,
        message_spacing: 1,
        wrap_width: 20,
    },
];

/// Attempts to parse and render a Mermaid diagram to Unicode box-drawing text.
///
/// Automatically tries density presets (default -> compact -> tight -> squeezed)
/// to fit the diagram into the available width before applying line clipping.
pub fn render_diagram(source: &str, width: usize) -> Option<String> {
    let renderer = Renderer::new();

    if width == 0 {
        return render_with_preset(&renderer, source, width, &PRESETS[0]);
    }

    let mut candidate: Option<String> = None;
    for preset in &PRESETS {
        if let Some(rendered) = render_with_preset(&renderer, source, width, preset) {
            let max_w = rendered
                .lines()
                .map(crate::ui::interactive::footer::visible_width)
                .max()
                .unwrap_or(0);

            if max_w <= width {
                return Some(rendered);
            }
            if candidate.is_none() {
                candidate = Some(rendered);
            }
        }
    }

    candidate
}

fn render_with_preset(renderer: &Renderer, source: &str, width: usize, preset: &Preset) -> Option<String> {
    let mut options = AsciiRenderOptions::unicode();
    options.color_mode = AsciiColorMode::Ansi16;
    options.graph_padding_x = preset.padding_x;
    options.graph_padding_y = preset.padding_y;
    options.box_border_padding = preset.box_border_padding;
    options.sequence_participant_spacing = preset.participant_spacing;
    options.sequence_message_spacing = preset.message_spacing;
    options.sequence_mirror_actors = true;
    options.flowchart_node_label_wrap_width = preset.wrap_width;

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
