//! Inline-element rendering (pulldown-cmark), mermaid blocks, and table formatting.

use crate::ui::theme::Theme;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
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
                let h = theme.code_inline;
                out.push_str(&format!("{h}`{c}`{h:#}"));
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

    let mut out = format!("\n{header}[mermaid diagram]{header:#}\n");
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
    out.push('\n');
    out
}

pub fn is_table_line(trimmed: &str) -> bool {
    (trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 2) || is_table_divider(trimmed)
}

pub fn is_table_divider(line: &str) -> bool {
    let stripped: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.starts_with('|')
        && stripped.ends_with('|')
        && stripped.len() >= 3
        && stripped.chars().all(|c| c == '|' || c == '-' || c == ':')
}

pub fn render_markdown_table(lines: &[String], _theme: &Theme) -> String {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);

    if let Ok((cols, _)) = crossterm::terminal::size() {
        let max_w = cols.saturating_sub(4).max(40);
        table.set_width(max_w);
    }

    let mut is_header = true;
    for line in lines {
        let trimmed = line.trim();
        if is_table_divider(trimmed) {
            is_header = false;
            continue;
        }

        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|s| strip_markdown_decorations(s.trim()))
            .collect();

        if is_header {
            table.set_header(cells);
            is_header = false;
        } else {
            table.add_row(cells);
        }
    }

    format!("\n{table}\n")
}

pub fn strip_markdown_decorations(s: &str) -> String {
    s.replace("**", "").replace(['*', '`'], "").trim().to_string()
}
