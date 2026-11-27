use reedline::{Prompt, PromptEditMode, PromptHistorySearch};
use std::borrow::Cow;

pub(crate) fn read_answer<R: std::io::BufRead, W: std::io::Write>(
    reader: &mut R,
    writer: &mut W,
    prompt: &str,
) -> Option<String> {
    write!(writer, "{prompt}").ok()?;
    writer.flush().ok()?;
    let mut input = String::new();
    reader.read_line(&mut input).ok()?;
    Some(input)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SimplePrompt;

impl Prompt for SimplePrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed(". ")
    }

    fn render_prompt_history_search_indicator(&self, _prompt_search: PromptHistorySearch) -> Cow<'_, str> {
        Cow::Borrowed("search: ")
    }
}
