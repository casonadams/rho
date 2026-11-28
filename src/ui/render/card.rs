use super::diff::replace_tabs;
use super::file_change;
use super::formatters::{EditDiffRequest, WritePreview, format_edit_diff, format_read_expanded, format_write_preview};
use crate::ui::interactive::truncate_to_visual_lines;
use crate::ui::theme::{BlockStyle, Theme};
use crate::ui::wrap::wrap_line_into;
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_bash_full_summary, format_tool_args_summary, read_summary_parts,
};
use rho_harness_core::presentation::{SUBAGENT_STARTED_MARKER, ToolLine};
use unicode_width::UnicodeWidthChar;

pub fn normalize_tool_name(name: &str) -> &str {
    match name {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    }
}

fn kind_from_format(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "pdf" => "pdf",
        "json" => "json",
        "csv" => "csv",
        "xml" => "xml",
        _ => "text",
    }
}

fn kind_from_url(url: &str) -> &'static str {
    if url.ends_with(".pdf") {
        "pdf"
    } else if url.ends_with(".json") {
        "json"
    } else if url.ends_with(".csv") {
        "csv"
    } else if url.ends_with(".xml") || url.ends_with(".rss") || url.ends_with(".atom") {
        "xml"
    } else {
        "text"
    }
}

pub fn fetch_content_kind(arguments: &serde_json::Value) -> &'static str {
    if let Some(format) = arguments.get("format").and_then(serde_json::Value::as_str) {
        return kind_from_format(format);
    }
    let url = arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    kind_from_url(&url)
}

pub fn detect_language_from_args(args: &serde_json::Value) -> Option<&str> {
    let path = rho_harness_core::args::target_path(args)?;
    detect_language_from_path(path)
}

pub fn detect_language_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path).extension()?.to_str()
}

pub(crate) fn split_bash_timeout(summary: &str) -> (&str, Option<&str>) {
    if let Some(idx) = summary.rfind(" (timeout ")
        && summary.ends_with(')')
    {
        let timeout_part = &summary[idx + 1..];
        let inner = &timeout_part["(timeout ".len()..timeout_part.len() - 1];
        if inner.ends_with('s') && inner[..inner.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            return (&summary[..idx], Some(timeout_part));
        }
    }
    (summary, None)
}

pub fn format_bash_args_header(summary: &str, accent: anstyle::Style, dim: anstyle::Style) -> String {
    match split_bash_timeout(summary) {
        (cmd, Some(timeout_part)) => format!("{accent}{cmd}{accent:#} {dim}{timeout_part}{dim:#}"),
        (cmd, None) => format!("{accent}{cmd}{accent:#}"),
    }
}

/// Syntax-highlights a `bash` tool header using `syntect` ANSI-16 colors while keeping any trailing
/// `(timeout Ns)` suffix dim.
pub fn highlight_bash_args_header(summary: &str, theme: &Theme) -> String {
    let dim = theme.dimmed;
    match split_bash_timeout(summary) {
        (cmd, Some(timeout_part)) => {
            let highlighted = crate::ui::markdown::highlight_code_line(cmd, Some("sh"), theme);
            format!("{highlighted} {dim}{timeout_part}{dim:#}")
        }
        (cmd, None) => crate::ui::markdown::highlight_code_line(cmd, Some("sh"), theme),
    }
}

pub fn format_read_header(tool: &ToolLine, theme: &Theme) -> String {
    let (path, range) = read_summary_parts(&tool.arguments);
    let title = theme.tool_title_style(false);
    let accent = theme.highlight;
    let range_suffix = range.map_or_else(String::new, |range| {
        let rs = theme.warning;
        format!("{rs}{range}{rs:#}")
    });
    match classify_read_path(&tool.arguments) {
        Some(ReadClassification::Skill { name }) => {
            let tag = theme.skill_tag;
            let offset = tool
                .arguments
                .get("offset")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1);
            let suffix = if offset <= 1 { "" } else { &range_suffix };
            format!("{tag}[skill]{tag:#} {name}{suffix}")
        }
        Some(ReadClassification::Resource { path }) => {
            format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        Some(ReadClassification::Docs { path }) => {
            format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        None => format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}"),
    }
}

pub fn format_tool_header(tool: &ToolLine, theme: &Theme) -> String {
    format_tool_header_with_expansion(tool, theme, false)
}

pub fn format_tool_header_with_expansion(tool: &ToolLine, theme: &Theme, expanded: bool) -> String {
    let mut header = format_tool_header_base(tool, theme, expanded);
    if let Some(details) = file_change::header_details(tool) {
        let dim = theme.dimmed;
        header.push_str(&format!("{dim} \u{b7} {details}{dim:#}"));
    }
    header
}

pub(crate) fn style_tool_header_args(tool_name: &str, summary: &str, theme: &Theme) -> String {
    if tool_name == "bash" {
        return if theme.syntax_highlight_tools {
            highlight_bash_args_header(summary, theme)
        } else {
            format_bash_args_header(summary, theme.highlight, theme.dimmed)
        };
    }
    if theme.syntax_highlight_tools && (summary.starts_with('{') || summary.starts_with('[')) {
        return crate::ui::markdown::highlight_code_line(summary, Some("json"), theme);
    }
    let accent = theme.highlight;
    format!("{accent}{summary}{accent:#}")
}

fn format_tool_header_base(tool: &ToolLine, theme: &Theme, expanded: bool) -> String {
    let title = theme.tool_title_style(tool.is_error);
    let accent = theme.highlight;
    let display_name = normalize_tool_name(&tool.name);
    if !tool.is_error && tool.name == "read" {
        return format_read_header(tool, theme);
    }
    if !tool.is_error && display_name == "web_fetch" {
        let url = tool
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let dim = theme.dimmed;
        let kind = fetch_content_kind(&tool.arguments);
        return format!("{title}web_fetch{title:#} {accent}{url}{accent:#} {dim}({kind}){dim:#}");
    }
    let summary = if tool.name == "bash" && (theme.full_bash_command || expanded) {
        format_bash_full_summary(&tool.arguments)
    } else {
        format_tool_args_summary(&tool.name, &tool.arguments)
    };
    if summary.is_empty() {
        format!("{title}{display_name}{title:#}")
    } else {
        let header_args = style_tool_header_args(&tool.name, &summary, theme);
        format!("{title}{display_name}{title:#} {header_args}")
    }
}

pub(crate) fn raw_output(tool: &ToolLine) -> &str {
    if tool.output.is_empty() {
        &tool.output_summary
    } else {
        &tool.output
    }
}

/// Adds `section` to the card body behind exactly one blank row, whatever newlines either side ends with.
pub(crate) fn push_section(content: &mut String, section: &str) {
    content.truncate(content.trim_end_matches('\n').len());
    content.push_str("\n\n");
    content.push_str(section.trim_end_matches('\n'));
}

/// Draws finished card content inside the tool block frame.
pub(crate) fn frame_tool_card(content: &str, is_bash: bool, is_error: bool, width: usize, theme: &Theme) -> String {
    theme
        .tool_block(is_bash, is_error, width)
        .with_vertical_padding()
        .render_styled(content)
}

fn append_read_expanded(content: &mut String, tool: &ToolLine, theme: &Theme) {
    if let Some(formatted) = format_read_expanded(raw_output(tool), &tool.arguments, theme) {
        push_section(content, &formatted);
    }
}

const OUTPUT_PREVIEW_ROWS: usize = 5;

/// Rows `line` wraps to in `wrap_to_width`, where a line that shows nothing takes a row only when it comes first.
fn wrapped_rows(line: &str, width: usize, first: bool) -> usize {
    let line = line.trim_end_matches('\r');
    if line.trim().is_empty() {
        return 1;
    }
    let columns: usize = line.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    if !line.contains('\x1b') && (1..=width).contains(&columns) {
        return 1;
    }
    let mut rows = if first { Vec::new() } else { vec![String::new()] };
    let seeded = rows.len();
    wrap_line_into(line, width, &mut rows);
    rows.len() - seeded
}

/// The first line of `text` the collapsed preview needs to fill its rows, and the rows above that line.
fn preview_start(text: &str, width: usize) -> (usize, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut first_line = lines.len();
    let mut shown_rows = 0;
    while first_line > 0 && shown_rows < OUTPUT_PREVIEW_ROWS {
        first_line -= 1;
        shown_rows += wrapped_rows(lines[first_line], width, first_line == 0);
    }
    let hidden_rows = lines[..first_line]
        .iter()
        .enumerate()
        .map(|(index, line)| wrapped_rows(line, width, index == 0))
        .sum();
    (first_line, hidden_rows)
}

/// The output body: all of `clean` when expanded, else its last rows under a notice. Only the lines
/// shown are highlighted and wrapped.
pub(crate) fn format_tool_output(tool_name: &str, clean: &str, width: usize, expanded: bool, theme: &Theme) -> String {
    let render_from = |first_line: usize| {
        if theme.syntax_highlight_tools {
            super::output_highlight::highlight_tool_output(tool_name, clean, first_line, theme)
        } else {
            clean[super::output_highlight::line_start(clean, first_line)..].to_string()
        }
    };
    if expanded {
        return render_from(0);
    }
    let row_width = width.saturating_sub(4).max(1);
    let (first_line, hidden_rows) = preview_start(clean, row_width);
    let truncated = truncate_to_visual_lines(&render_from(first_line), OUTPUT_PREVIEW_ROWS, row_width);
    let hidden = hidden_rows + truncated.skipped_count;
    let mut out = super::formatters::earlier_lines_notice(hidden, None, row_width, theme.dimmed);
    out.push_str(&truncated.visual_lines.join("\n"));
    out
}

fn append_generic_output(content: &mut String, tool: &ToolLine, width: usize, expanded: bool, theme: &Theme) {
    let clean = replace_tabs(raw_output(tool).trim_end());
    if !clean.is_empty() {
        push_section(content, &format_tool_output(&tool.name, &clean, width, expanded, theme));
    }
}

fn edit_diff(tool: &ToolLine, expanded: bool, width: usize, theme: &Theme) -> Option<String> {
    let request = EditDiffRequest {
        args: &tool.arguments,
        file_change: tool.file_change.as_ref(),
        expanded,
        width: theme.tool_block(false, false, width).inner_width(),
    };
    format_edit_diff(request, theme)
}

pub(crate) fn append_failed_edit_attempt(
    content: &mut String,
    tool: &ToolLine,
    view: CardView,
    width: usize,
    theme: &Theme,
) {
    if let Some(diff) = edit_diff(tool, view.expanded, width, theme) {
        let dim = theme.dimmed;
        push_section(content, &format!("{dim}Attempted change:{dim:#}\n{diff}"));
    }
}

pub(crate) fn shows_file_change(tool: &ToolLine) -> bool {
    !tool.is_error && matches!(tool.name.as_str(), "edit" | "write")
}

/// The diff of a successful edit or the content preview of a successful write.
pub(crate) fn file_change_preview(tool: &ToolLine, view: CardView, width: usize, theme: &Theme) -> Option<String> {
    if tool.name == "write" {
        let preview = WritePreview {
            args: &tool.arguments,
            dropped_lines: view.dropped_lines,
            expanded: view.expanded,
            width: theme.tool_block(false, false, width).inner_width(),
        };
        return format_write_preview(preview, theme);
    }
    edit_diff(tool, view.expanded, width, theme)
}

fn append_edit_or_write(content: &mut String, tool: &ToolLine, width: usize, view: CardView, theme: &Theme) -> bool {
    if !shows_file_change(tool) {
        return false;
    }
    if let Some(preview) = file_change_preview(tool, view, width, theme) {
        push_section(content, &preview);
    }
    true
}

pub(crate) fn append_background_hint(content: &mut String, tool: &ToolLine, dim: anstyle::Style) {
    if tool.name == "subagent" && !tool.is_error && raw_output(tool).starts_with(SUBAGENT_STARTED_MARKER) {
        push_section(
            content,
            &format!("{dim}running in the background - press \u{2190} on an empty prompt to watch{dim:#}"),
        );
    }
}

pub(crate) fn append_tool_details(content: &mut String, tool: &ToolLine, width: usize, view: CardView, theme: &Theme) {
    append_tool_body(content, tool, width, view, theme);
    if let Some(duration_ms) = tool.duration_ms {
        push_section(content, &duration_row(duration_ms, view.status, theme.dimmed));
    }
}

/// The details under the header, all but the duration row.
pub(crate) fn append_tool_body(content: &mut String, tool: &ToolLine, width: usize, view: CardView, theme: &Theme) {
    if !tool.is_error && tool.name == "read" {
        if view.show_read_expanded {
            append_read_expanded(content, tool, theme);
        }
    } else if !append_edit_or_write(content, tool, width, view, theme) && shows_output_body(tool, view) {
        append_generic_output(content, tool, width, view.expanded, theme);
    }
    if tool.is_error && tool.name == "edit" {
        append_failed_edit_attempt(content, tool, view, width, theme);
    }
    append_background_hint(content, tool, theme.dimmed);
}

pub(crate) fn duration_row(duration_ms: u64, status: CardStatus, dim: anstyle::Style) -> String {
    let label = match status {
        CardStatus::Running => "Elapsed",
        CardStatus::Finished => "Took",
    };
    format!("{dim}{label} {}{dim:#}", super::format_duration_ms(duration_ms))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Running,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardView {
    pub status: CardStatus,
    pub expanded: bool,
    pub show_read_expanded: bool,
    /// Lines a running card's streamed content has dropped from its front.
    pub dropped_lines: usize,
}

pub(crate) fn shows_output_body(tool: &ToolLine, view: CardView) -> bool {
    tool.name == "bash" || tool.is_error || view.expanded || view.status == CardStatus::Running
}

pub fn render_tool_block(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> String {
    let view = CardView {
        status: CardStatus::Finished,
        expanded,
        show_read_expanded,
        dropped_lines: 0,
    };
    render_card(tool, view, width, theme)
}

pub(super) fn render_card(tool: &ToolLine, view: CardView, width: usize, theme: &Theme) -> String {
    let content = if theme.structured_tool_cards {
        super::structured::render_structured_tool_content(tool, view, width, theme)
    } else {
        let mut content = format_tool_header_with_expansion(tool, theme, view.expanded);
        append_tool_details(&mut content, tool, width, view, theme);
        content
    };
    frame_tool_card(&content, tool.name == "bash", tool.is_error, width, theme)
}

pub fn render_tool_transcript(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> String {
    let block = render_tool_block(tool, width, expanded, show_read_expanded, theme);
    if theme.block_style == BlockStyle::Border {
        block
    } else {
        format!("\n{block}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The collapsed body as it was drawn by highlighting and wrapping the whole output.
    fn whole_output_preview(tool_name: &str, clean: &str, width: usize, theme: &Theme) -> String {
        let rendered = if theme.syntax_highlight_tools {
            super::super::output_highlight::highlight_tool_output(tool_name, clean, 0, theme)
        } else {
            clean.to_string()
        };
        let row_width = width.saturating_sub(4).max(1);
        let truncated = truncate_to_visual_lines(&rendered, OUTPUT_PREVIEW_ROWS, row_width);
        let mut out =
            super::super::formatters::earlier_lines_notice(truncated.skipped_count, None, row_width, theme.dimmed);
        out.push_str(&truncated.visual_lines.join("\n"));
        out
    }

    fn sample_outputs() -> Vec<(&'static str, String)> {
        let log: String = (1..=200)
            .map(|i| format!("step {i}: compiling crate number {i}\n"))
            .collect();
        let json = serde_json::to_string_pretty(
            &serde_json::json!({"items": vec![serde_json::json!({"k": "v", "n": [1, true, null]}); 40]}),
        )
        .unwrap_or_default();
        let words = "lorem ipsum dolor sit amet ".repeat(30);
        vec![
            ("bash", log.trim_end().to_string()),
            ("bash", format!("{log}{words}")),
            ("bash", format!("short\n\n{words}\n\nend")),
            ("bash", words.trim_end().to_string()),
            ("bash", "one\ntwo\nthree".to_string()),
            ("bash", "1\n2\n3\n4\n5".to_string()),
            ("bash", "crlf one\r\ncrlf two\r\n\r\ncrlf three".to_string()),
            ("bash", format!("\x1b[32m{log}\x1b[0m")),
            ("mcp", json),
            (
                "bash",
                format!("{log}```rust\nfn main() {{\n    let x = 1;\n}}\n```\ndone"),
            ),
            (
                "bash",
                format!(
                    "diff --git a/x b/x\n@@ -1 +1 @@\n{}",
                    "-old\n+new\n".repeat(20).trim_end()
                ),
            ),
            (
                "rg",
                format!(
                    "src/a.rs\n{}",
                    (1..40)
                        .map(|i| format!("{i}:let v{i} = {i};"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            ),
            (
                "fd",
                (0..30).map(|i| format!("  dir{i}/")).collect::<Vec<_>>().join("\n"),
            ),
            (
                "lsp",
                (0..30)
                    .map(|i| format!("src/a.rs:{i}\nerror: e{i}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ]
    }

    #[test]
    fn collapsed_output_matches_the_whole_output_preview() {
        for syntax_highlight_tools in [false, true] {
            let theme = Theme {
                syntax_highlight_tools,
                ..Theme::default()
            };
            for (tool, clean) in sample_outputs() {
                for width in [12, 40, 80, 200] {
                    assert_eq!(
                        format_tool_output(tool, &clean, width, false, &theme),
                        whole_output_preview(tool, &clean, width, &theme),
                        "{tool} width {width} highlight {syntax_highlight_tools}: {clean:?}"
                    );
                }
            }
        }
    }
}
