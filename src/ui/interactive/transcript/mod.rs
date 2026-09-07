mod skill;
mod tool;
pub mod types;
pub mod welcome;

#[cfg(test)]
mod tests;

pub use tool::render_tool_block;
pub use types::{
    OSC133_ZONE_END, OSC133_ZONE_FINAL, OSC133_ZONE_START, ToolItem, TranscriptItem, TranscriptRenderInput, WelcomeItem,
};
pub use welcome::format_welcome_content;

use crate::ui::render::format_thinking_block;

fn render_assistant_text(text: &str, width: usize, theme: &crate::ui::theme::Theme) -> String {
    let mut md = crate::ui::markdown::MarkdownRenderer::default();
    md.set_width(width);
    let full = format!("{}{}", md.render_token(text, theme), md.flush(theme));
    if full.trim().is_empty() {
        String::new()
    } else {
        format!("{OSC133_ZONE_START}\n{full}{OSC133_ZONE_END}{OSC133_ZONE_FINAL}")
    }
}

fn render_thinking_text(text: &str, hide_thinking: bool, theme: &crate::ui::theme::Theme) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        String::new()
    } else if hide_thinking {
        let dim = theme.dimmed;
        format!("\n{dim}Thinking...{dim:#}\n")
    } else {
        format_thinking_block(trimmed, theme)
    }
}

pub fn render_transcript_item(mut input: TranscriptRenderInput<'_>) -> String {
    input.width = input.width.max(20);
    match input.item {
        TranscriptItem::Welcome(welcome) => format_welcome_content(welcome, input.theme),
        TranscriptItem::UserMessage(text) => skill::render_user_message(text, &input),
        TranscriptItem::AssistantText(text) => render_assistant_text(text, input.width, input.theme),
        TranscriptItem::Thinking(text) => render_thinking_text(text, input.hide_thinking, input.theme),
        TranscriptItem::Tool(tool) => tool::render_tool_transcript(tool, &input),
        TranscriptItem::Notice(text) => text.clone(),
    }
}
