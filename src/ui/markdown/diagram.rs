//! Mermaid diagram rendering via headless Merman ASCII/Unicode engine.

use crate::ui::theme::Theme;
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

pub fn render_mermaid_block(source: &str, theme: &Theme, width: usize) -> String {
    let dim = theme.dimmed;

    if let Some(rendered) = render_diagram(source, width) {
        let mut out = format!("{dim}Mermaid{dim:#}\n");
        for line in rendered.lines() {
            out.push(' ');
            out.push_str(&clipped(line, width.saturating_sub(1)));
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
        return out;
    }

    let mut out = format!("{dim}```mermaid{dim:#}\n");
    for line in source.lines() {
        out.push_str(&clipped(line, width));
        out.push('\n');
    }
    out.push_str(&format!("{dim}```{dim:#}"));
    out
}

fn clipped(line: &str, width: usize) -> String {
    if width == 0 {
        line.to_string()
    } else {
        crate::ui::interactive::footer::truncate_to_width(line, width)
    }
}

#[derive(Default)]
pub struct MermaidBlockTracker {
    in_block: bool,
    lines: Vec<String>,
    width: usize,
}

impl MermaidBlockTracker {
    pub fn in_block(&self) -> bool {
        self.in_block
    }

    pub fn push_line(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }

    pub fn set_width(&mut self, width: usize) {
        self.width = width;
    }

    pub fn try_render_fence(&mut self, trimmed: &str, theme: &Theme) -> Option<Option<String>> {
        if !trimmed.starts_with("```") {
            return None;
        }
        let tag = trimmed.trim_start_matches('`').trim();
        if self.in_block {
            self.in_block = false;
            let src = std::mem::take(&mut self.lines).join("\n");
            Some(Some(render_mermaid_block(&src, theme, self.width)))
        } else if tag.eq_ignore_ascii_case("mermaid") {
            self.in_block = true;
            self.lines.clear();
            Some(None)
        } else {
            None
        }
    }

    pub fn flush_rendered(&mut self, theme: &Theme) -> Option<String> {
        if self.in_block && !self.lines.is_empty() {
            self.in_block = false;
            Some(render_mermaid_block(
                &std::mem::take(&mut self.lines).join("\n"),
                theme,
                self.width,
            ))
        } else {
            self.in_block = false;
            None
        }
    }
}
