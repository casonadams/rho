//! The card of a running tool, drawn once per change to the tool and re-framed per tick only where
//! its clock shows.

use super::card::{
    CardStatus, CardView, append_tool_body, duration_row, format_tool_header_with_expansion, frame_tool_card,
};
use super::structured::{align_header_with_right, structured_card_parts};
use crate::ui::theme::Theme;
use rho_harness_core::presentation::ToolLine;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Clock {
    /// An `Elapsed` row under the details, behind a blank row.
    Footer,
    /// The duration right-aligned on the structured header.
    Header { left: String, inner_width: usize },
}

/// A running tool's framed card without its clock. Framing is line by line, so the card is the
/// frame's opening row, the clock's rows and the cached rows around them, and the closing row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveCard {
    is_bash: bool,
    is_error: bool,
    width: usize,
    clock: Clock,
    /// The framed rows of the content the clock does not change.
    rows: String,
}

/// The rows the frame opens and closes with, which is all it draws around no content.
fn frame_edges(card: &LiveCard, theme: &Theme) -> (String, String) {
    let mut opening = frame_tool_card("", card.is_bash, card.is_error, card.width, theme);
    let closing = opening.split_off(opening.find('\n').map_or(opening.len(), |end| end + 1));
    (opening, closing)
}

/// The rows `content` frames to, without the frame's opening and closing rows.
fn framed_rows(content: &str, card: &LiveCard, theme: &Theme) -> String {
    let (opening, closing) = frame_edges(card, theme);
    let framed = frame_tool_card(content, card.is_bash, card.is_error, card.width, theme);
    framed[opening.len()..framed.len() - closing.len()].to_string()
}

impl LiveCard {
    /// The live card of a running tool, whose `duration_ms` it leaves to `render`.
    /// `dropped_lines` counts the lines its streamed output no longer holds.
    pub(crate) fn new(tool: &ToolLine, dropped_lines: usize, width: usize, expanded: bool, theme: &Theme) -> Self {
        let view = CardView {
            status: CardStatus::Running,
            expanded,
            show_read_expanded: expanded,
            dropped_lines,
        };
        let mut card = Self {
            is_bash: tool.name == "bash",
            is_error: tool.is_error,
            width,
            clock: Clock::Footer,
            rows: String::new(),
        };
        let content = if theme.structured_tool_cards {
            let parts = structured_card_parts(tool, view, width, theme);
            card.clock = Clock::Header {
                left: parts.header_left,
                inner_width: parts.inner_width,
            };
            parts.sections.strip_prefix('\n').unwrap_or_default().to_string()
        } else {
            let mut content = format_tool_header_with_expansion(tool, theme, expanded);
            append_tool_body(&mut content, tool, width, view, theme);
            content.trim_end_matches('\n').to_string()
        };
        card.rows = framed_rows(&content, &card, theme);
        card
    }

    /// The card with `duration_ms` on its clock.
    pub(crate) fn render(&self, duration_ms: u64, theme: &Theme) -> String {
        let (opening, closing) = frame_edges(self, theme);
        let clock = match &self.clock {
            Clock::Footer => {
                let row = duration_row(duration_ms, CardStatus::Running, theme.dimmed);
                format!("{}{}", self.rows, framed_rows(&format!("\n{row}"), self, theme))
            }
            Clock::Header { left, inner_width } => {
                let duration = super::format_duration_ms(duration_ms);
                let header = align_header_with_right(left, Some(&duration), *inner_width, theme.dimmed);
                format!(
                    "{}{}",
                    framed_rows(header.trim_end_matches('\n'), self, theme),
                    self.rows
                )
            }
        };
        format!("{opening}{clock}{closing}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::render::card::render_card;
    use crate::ui::theme::BlockStyle;
    use serde_json::json;

    fn sample_tools() -> Vec<ToolLine> {
        let log: String = (1..=40).map(|i| format!("compiling crate {i} of 40\n")).collect();
        let module: String = (0..30)
            .map(|i| format!("pub fn f{i}() -> u32 {{\n    {i}\n}}\n"))
            .collect();
        vec![
            ToolLine::new("bash", json!({"command": "cargo build", "timeout": 60})).with_output(log),
            ToolLine::new("bash", json!({"command": "true"})),
            ToolLine::new("write", json!({"path": "src/lib.rs", "text": module})),
            ToolLine::new(
                "edit",
                json!({"path": "src/lib.rs", "edits": [{"oldText": "a", "newText": "b", "line": 3}]}),
            ),
            ToolLine::new("subagent", json!({"task": "look around"})).with_output("working\nstill working"),
        ]
    }

    #[test]
    fn a_live_card_renders_as_the_whole_running_card_does() {
        for structured_tool_cards in [false, true] {
            for block_style in [BlockStyle::Solid, BlockStyle::Border, BlockStyle::Divider] {
                let theme = Theme {
                    structured_tool_cards,
                    block_style,
                    syntax_highlight_tools: true,
                    ..Theme::default()
                };
                for tool in sample_tools() {
                    for (width, expanded, duration_ms) in [(40, false, 12), (80, false, 1_500), (100, true, 61_000)] {
                        let view = CardView {
                            status: CardStatus::Running,
                            expanded,
                            show_read_expanded: expanded,
                            dropped_lines: 7,
                        };
                        let whole = render_card(&tool.clone().with_duration_ms(duration_ms), view, width, &theme);
                        let live = LiveCard::new(&tool, 7, width, expanded, &theme).render(duration_ms, &theme);
                        assert_eq!(
                            live, whole,
                            "{} {block_style:?} structured {structured_tool_cards}",
                            tool.name
                        );
                    }
                }
            }
        }
    }
}
