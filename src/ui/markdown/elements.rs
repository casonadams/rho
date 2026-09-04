//! Inline-element rendering (pulldown-cmark) and mermaid diagram blocks.

use crate::ui::theme::Theme;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub fn render_inline_elements(text: &str, theme: &Theme) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(text, options);
    let mut out = String::new();

    let bold_style = anstyle::Style::new().bold();
    let italic_style = anstyle::Style::new().italic();
    let strike_style = anstyle::Style::new().strikethrough();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => out.push_str(&bold_style.render().to_string()),
                Tag::Emphasis => out.push_str(&italic_style.render().to_string()),
                Tag::Strikethrough => out.push_str(&strike_style.render().to_string()),
                Tag::Link { .. } => out.push_str(&theme.highlight.render().to_string()),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => out.push_str(&bold_style.render_reset().to_string()),
                TagEnd::Emphasis => out.push_str(&italic_style.render_reset().to_string()),
                TagEnd::Strikethrough => out.push_str(&strike_style.render_reset().to_string()),
                TagEnd::Link => out.push_str(&theme.highlight.render_reset().to_string()),
                _ => {}
            },
            Event::Text(t) => out.push_str(&t),
            Event::Code(c) => {
                let code = theme.code_inline;
                out.push_str(&format!("{code}{c}{code:#}"));
            }
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }

    let trailing_spaces = text.len() - text.trim_end_matches(' ').len();
    if trailing_spaces > 0 && !out.ends_with(' ') {
        for _ in 0..trailing_spaces {
            out.push(' ');
        }
    }

    out
}

pub fn render_mermaid_block(source: &str, theme: &Theme) -> String {
    let header = theme.tool_header;
    let dim = theme.dimmed;

    let mut out = format!("{header}[mermaid diagram]{header:#}\n");
    match meraid::render(source, meraid::ThemeType::default()) {
        Ok(rendered) => {
            for line in rendered.lines() {
                out.push_str(&format!("{line}\n"));
            }
        }
        Err(_) => {
            for line in source.lines() {
                out.push_str(&format!("{dim}│{dim:#} {line}\n"));
            }
        }
    }
    out
}
