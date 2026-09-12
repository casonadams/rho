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
        Event::Html(h) | Event::InlineHtml(h) => {
            let dim = theme.dimmed;
            out.push_str(&format!("{dim}{h}{dim:#}"));
        }
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
    let dim = theme.dimmed;

    if let Some(rendered) = super::diagram::render_diagram(source, width) {
        let mut out = String::new();
        for line in rendered.lines() {
            out.push_str(&clipped(line, width));
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
