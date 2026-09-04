mod skill;
mod tool;
pub mod types;
pub mod welcome;

#[cfg(test)]
mod tests;

pub use types::{
    OSC133_ZONE_END, OSC133_ZONE_FINAL, OSC133_ZONE_START, ToolItem, TranscriptItem, TranscriptRenderInput, WelcomeItem,
};
pub use welcome::format_welcome_content;

use crate::ui::render::format_thinking_block;

pub fn render_transcript_item(mut input: TranscriptRenderInput<'_>) -> String {
    input.width = input.width.max(20);
    match input.item {
        TranscriptItem::Welcome(welcome) => format_welcome_content(welcome, input.theme),
        TranscriptItem::UserMessage(text) => skill::render_user_message(text, &input),
        TranscriptItem::AssistantText(text) => {
            let mut md = crate::ui::markdown::MarkdownRenderer::default();
            let rendered = md.render_token(text, input.theme);
            let flushed = md.flush(input.theme);
            let full = format!("{rendered}{flushed}");
            if full.trim().is_empty() {
                String::new()
            } else {
                format!("{OSC133_ZONE_START}\n{full}{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")
            }
        }
        TranscriptItem::Thinking(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                String::new()
            } else if input.hide_thinking {
                let dim = input.theme.dimmed;
                format!("\n{dim}Thinking...{dim:#}\n")
            } else {
                format_thinking_block(trimmed, input.theme)
            }
        }
        TranscriptItem::Tool(tool) => tool::render_tool_transcript(tool, &input),
        TranscriptItem::Notice(text) => text.clone(),
    }
}
