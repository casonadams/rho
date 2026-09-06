//! Inline-element rendering (pulldown-cmark) and mermaid diagram blocks.

use crate::ui::theme::Theme;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn tag_start_style(tag: Tag, theme: &Theme) -> Option<String> {
    match tag {
        Tag::Strong => Some(anstyle::Style::new().bold().render().to_string()),
        Tag::Emphasis => Some(anstyle::Style::new().italic().render().to_string()),
        Tag::Strikethrough => Some(anstyle::Style::new().strikethrough().render().to_string()),
        Tag::Link { .. } => Some(theme.highlight.render().to_string()),
        _ => None,
    }
}

fn tag_end_style(tag_end: TagEnd, theme: &Theme) -> Option<String> {
    match tag_end {
        TagEnd::Strong => Some(anstyle::Style::new().bold().render_reset().to_string()),
        TagEnd::Emphasis => Some(anstyle::Style::new().italic().render_reset().to_string()),
        TagEnd::Strikethrough => Some(anstyle::Style::new().strikethrough().render_reset().to_string()),
        TagEnd::Link => Some(theme.highlight.render_reset().to_string()),
        _ => None,
    }
}

fn push_inline_event(out: &mut String, event: Event, theme: &Theme) {
    match event {
        Event::Start(tag) => {
            if let Some(s) = tag_start_style(tag, theme) {
                out.push_str(&s);
            }
        }
        Event::End(tag_end) => {
            if let Some(s) = tag_end_style(tag_end, theme) {
                out.push_str(&s);
            }
        }
        Event::Text(t) => out.push_str(&t),
        Event::Code(c) => out.push_str(&format!("{}{c}{:#}", theme.code_inline, theme.code_inline)),
        Event::SoftBreak => out.push(' '),
        Event::HardBreak => out.push('\n'),
        _ => {}
    }
}

fn restore_trailing_spaces(out: &mut String, text: &str) {
    let trailing_spaces = text.len() - text.trim_end_matches(' ').len();
    if trailing_spaces > 0 && !out.ends_with(' ') {
        out.push_str(&" ".repeat(trailing_spaces));
    }
}

pub fn render_inline_elements(text: &str, theme: &Theme) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut out = String::new();
    for event in Parser::new_ext(text, options) {
        push_inline_event(&mut out, event, theme);
    }
    restore_trailing_spaces(&mut out, text);
    out
}

pub fn render_mermaid_block(source: &str, theme: &Theme, width: usize) -> String {
    let (header, dim) = (theme.tool_header, theme.dimmed);
    let mut out = format!("{header}[mermaid diagram]{header:#}\n");
    match meraid::render(source, meraid::ThemeType::default()) {
        Ok(rendered) => {
            for line in rendered.lines() {
                out.push_str(&clipped(line, width));
                out.push('\n');
            }
        }
        Err(_) => {
            for line in source.lines() {
                out.push_str(&format!("{dim}│{dim:#} {}", clipped(line, width.saturating_sub(2))));
                out.push('\n');
            }
        }
    }
    out
}

fn clipped(line: &str, width: usize) -> String {
    if width == 0 {
        line.to_string()
    } else {
        crate::ui::interactive::footer::truncate_to_width(line, width)
    }
}
