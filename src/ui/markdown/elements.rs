//! Inline-element rendering (pulldown-cmark).

use crate::ui::theme::Theme;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

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

#[derive(Default)]
struct InlineState {
    links: Vec<(String, usize)>,
}

fn start_link(text_start: usize, state: &mut InlineState, link_type: LinkType, dest: &str) {
    let shown_as_text = matches!(link_type, LinkType::Autolink | LinkType::Email);
    let dest = if shown_as_text { String::new() } else { dest.to_string() };
    state.links.push((dest, text_start));
}

fn end_link(out: &mut String, state: &mut InlineState, theme: &Theme) {
    let Some((dest, text_start)) = state.links.pop() else {
        return;
    };
    let text = crate::ui::block::strip_ansi(&out[text_start..]);
    if !dest.is_empty() && text.trim() != dest {
        let dim = theme.dimmed;
        out.push_str(&format!(" {dim}({dest}){dim:#}"));
    }
}

fn push_inline_event(out: &mut String, state: &mut InlineState, event: Event, theme: &Theme) {
    match event {
        Event::Start(Tag::Subscript) | Event::End(TagEnd::Subscript) => out.push('~'),
        Event::Start(tag) => {
            if let Tag::Link {
                link_type, dest_url, ..
            } = &tag
            {
                start_link(out.len(), state, *link_type, dest_url);
            }
            if let Some(s) = tag_start_style(tag, theme) {
                out.push_str(&s);
            }
        }
        Event::End(tag_end) => {
            let is_link = tag_end == TagEnd::Link;
            if let Some(s) = tag_end_style(tag_end, theme) {
                out.push_str(&s);
            }
            if is_link {
                end_link(out, state, theme);
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
    options.insert(Options::ENABLE_SUBSCRIPT);

    let mut out = String::new();
    let mut state = InlineState::default();
    for event in Parser::new_ext(text, options) {
        push_inline_event(&mut out, &mut state, event, theme);
    }
    restore_trailing_spaces(&mut out, text);
    out
}
