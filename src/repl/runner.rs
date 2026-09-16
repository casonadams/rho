use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use rho_harness_core::rpc::protocol::RpcEvent;
use rho_harness_core::session::list_session_summaries_async;
use rho_ui_core::autocomplete::CompletionEngine;
use rho_ui_core::keymap::{InputAction, map_key};
use rho_ui_core::modal::{
    McpModalState, McpServerInfo, ModalOption, ModalState, ModelCapability, ModelRegistry, SettingsState, SkillInfo,
    SkillModalState,
};
use rho_ui_core::state::FooterMetrics;

use crate::engine::AgentEngine;
use crate::engine::runner::{CancellationSignal, TurnRequest};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::coordinator::SharedSteeringQueue;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::PromptEditor;
use crate::ui::editor::{EditorMode, TextAreaEditor};
use crate::ui::interactive::{
    Activity, InteractionPrompt, InteractionResponder, InteractionResponse, InteractiveUi, OutputEvent, RunningTool,
    TranscriptItem, TranscriptRenderInput, UiEvent,
};
use crate::ui::modal::{AutocompletePopupView, StandardModalView, run_modal_view};
use crate::ui::terminal::TerminalGuard;
use crate::ui::theme::CursorMode;
use crate::ui::widgets::StreamingSpinner;

const CSI_SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const CSI_SYNC_END: &[u8] = b"\x1b[?2026l";
const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

fn apply_software_cursor(line: &mut String, target_column: usize) {
    let mut current_col = 0;
    let mut byte_offset = None;
    let mut char_len = 0;

    for (idx, ch) in line.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_col == target_column || (cw > 1 && target_column > current_col && target_column < current_col + cw) {
            byte_offset = Some(idx);
            char_len = ch.len_utf8();
            break;
        }
        current_col += cw;
    }

    if let Some(offset) = byte_offset {
        let before = &line[..offset];
        let ch_str = &line[offset..offset + char_len];
        let after = &line[offset + char_len..];
        *line = format!("{before}\x1b[7m{ch_str}\x1b[27m{after}");
    } else {
        line.push_str("\x1b[7m \x1b[27m");
    }
}

fn window_widget_lines(lines: &[String], budget: usize) -> Vec<String> {
    if lines.len() <= budget {
        return lines.to_vec();
    }
    if budget == 0 {
        return Vec::new();
    }
    let has_borders = lines.first().is_some_and(|l| l.contains('╭')) && lines.last().is_some_and(|l| l.contains('╰'));
    if has_borders && budget >= 3 {
        let top_lines = 2.min(budget.saturating_sub(1));
        let bottom_lines = 1;
        let interior_budget = budget.saturating_sub(top_lines + bottom_lines);
        let interior = &lines[top_lines..lines.len() - bottom_lines];
        let mut result = Vec::with_capacity(budget);
        result.extend_from_slice(&lines[..top_lines]);
        if interior.len() > interior_budget {
            result.extend_from_slice(&interior[interior.len() - interior_budget..]);
        } else {
            result.extend_from_slice(interior);
        }
        result.extend_from_slice(&lines[lines.len() - bottom_lines..]);
        result
    } else {
        lines.iter().rev().take(budget).rev().cloned().collect()
    }
}

fn render_running_tool_widget(
    tool: &RunningTool,
    theme: &crate::ui::Theme,
    width: usize,
    tools_expanded: bool,
) -> Vec<String> {
    if tool.preview.is_none() && tool.output.is_empty() && tool.name != "bash" {
        return Vec::new();
    }
    let width = width.max(20);
    let title = theme.tool_title_style(false);
    let (accent, dim) = (theme.highlight, theme.dimmed);
    let display_name = match tool.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };
    let args_header = if tool.name == "bash" {
        crate::ui::render::format_bash_args_header(&tool.args_summary, accent, dim)
    } else {
        format!("{accent}{}{accent:#}", tool.args_summary)
    };
    let mut content = format!("{title}{display_name}{title:#} {args_header}");
    if let Some(preview) = &tool.preview {
        content.push_str("\n\n");
        content.push_str(preview);
    }
    let raw_output = tool.output.trim_end().replace('\t', "   ");
    if !raw_output.is_empty() {
        content.push_str("\n\n");
        if tools_expanded {
            content.push_str(&raw_output);
        } else {
            let truncated = crate::ui::block::truncate_to_visual_lines(&raw_output, 5, width.saturating_sub(4).max(1));
            if truncated.skipped_count > 0 {
                content.push_str(&format!(
                    "{dim}... ({} earlier lines){dim:#}\n",
                    truncated.skipped_count
                ));
            }
            content.push_str(&truncated.visual_lines.join("\n"));
        }
    }
    content.push_str(&format!(
        "\n\n{dim}Elapsed {}{dim:#}",
        rho_ui_core::format_duration(tool.elapsed())
    ));
    let block = theme
        .tool_block(tool.name == "bash", false, width)
        .with_vertical_padding()
        .render_styled(&content);
    let mut lines = if theme.block_style == crate::ui::theme::BlockStyle::Border {
        Vec::new()
    } else {
        vec![String::new()]
    };
    lines.extend(block.lines().map(String::from));
    lines
}

#[derive(Clone, Default)]
pub(crate) struct FooterInfo {
    pub model: String,
    pub provider: String,
    pub thinking: Option<String>,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    pub context_window: usize,
    pub context_percent: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub quota: Option<String>,
    pub path_left: String,
}

fn make_footer_info(model: &str, provider: &str, thinking: Option<&str>, engine: &AgentEngine) -> FooterInfo {
    let current_dir = std::env::current_dir().unwrap_or_default();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from);
    let mut path = rho_ui_core::footer::abbreviate_home(&current_dir, home.as_deref());
    if let Some(branch) = rho_ui_core::footer::get_git_branch(&current_dir)
        && !branch.is_empty()
    {
        path.push_str(&format!(" ({branch})"));
    }
    let totals = engine.session_usage_totals();
    let context_window = engine.context_limit().unwrap_or(0);
    let context_percent = engine.context_percent_f64();
    let tokens_per_second = engine.tokens_per_second();
    let quota = engine.quota_display();
    FooterInfo {
        model: model.to_string(),
        provider: provider.to_string(),
        thinking: thinking.map(str::to_string),
        total_input: totals.total_input,
        total_output: totals.total_output,
        total_cache_read: totals.total_cache_read,
        total_cache_write: totals.total_cache_write,
        context_window,
        context_percent,
        tokens_per_second,
        quota,
        path_left: path,
    }
}

fn output_cursor(value: &str, terminal_width: usize) -> (usize, bool) {
    let mut col = 0;
    let mut at_wrap = false;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if (0x40..=0x7E).contains(&(next as u8)) {
                        break;
                    }
                }
            }
        } else if c == '\r' {
            col = 0;
            at_wrap = false;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if col + w > terminal_width {
                col = w;
                at_wrap = true;
            } else {
                col += w;
                at_wrap = col == terminal_width;
            }
        }
    }
    (col, at_wrap)
}

fn terminal_newlines(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut prev_cr = false;
    for c in value.chars() {
        if c == '\n' && !prev_cr {
            result.push('\r');
        }
        result.push(c);
        prev_cr = c == '\r';
    }
    result
}

#[derive(Debug, Default)]
struct OutputTracker {
    line: String,
    open: bool,
}

impl OutputTracker {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn clear(&mut self) {
        self.line.clear();
        self.open = false;
    }

    pub fn update(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        if let Some(newline) = output.rfind('\n') {
            self.line.clear();
            self.line.push_str(&output[newline + 1..]);
        } else {
            self.line.push_str(output);
        }
        let has_newline =
            output.ends_with('\n') || (output.rfind('\n').is_some() && output_cursor(&self.line, usize::MAX).0 == 0);
        self.open = !has_newline;
        if !self.open {
            self.line.clear();
        }
    }

    pub fn restore_cursor(&self, stdout: &mut std::io::Stdout, width: usize) -> std::io::Result<()> {
        if !self.open {
            return Ok(());
        }
        let (column, at_wrap_boundary) = output_cursor(&self.line, width);
        if !at_wrap_boundary {
            stdout.write_all(b"\x1b[1A")?;
        }
        write!(stdout, "\r\x1b[{column}C")?;
        Ok(())
    }
}

struct RunnerState<'a> {
    pub session: &'a mut ReplSession,
    pub editor: &'a mut TextAreaEditor,
    pub history: &'a mut InteractiveHistory,
    pub autocomplete_popup: &'a mut Option<AutocompletePopupView>,
    pub transcript: &'a mut Vec<TranscriptItem>,
    pub tracker: &'a mut OutputTracker,
    pub prev_lines_count: usize,
    pub prev_cursor_row: usize,
}

async fn init_live_engine(session: &mut ReplSession) -> Result<AgentEngine> {
    let engine = crate::platform::agent_engine(
        session.config.clone(),
        session.auth_store.clone(),
        session.resume_id.as_deref(),
    )
    .await?;

    if let Some(ref cli) = session.cli
        && let Some(ref name) = cli.name
    {
        let _ = engine.session_manager.set_session_name(name).await;
    }

    session.config = engine.config.clone();
    engine.refresh_quota().await;
    Ok(engine)
}

async fn build_welcome_item(session: &ReplSession, engine: &AgentEngine) -> crate::ui::interactive::WelcomeItem {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let tools = engine.tool_names();
    let statuses = rho_engine::mcp::get_mcp_server_statuses();
    let mcp = session
        .config
        .mcp
        .servers
        .keys()
        .map(|name| {
            if let Some(st) = statuses.get(name) {
                if st.error.is_some() || !st.is_loaded {
                    format!("{name} (failed)")
                } else {
                    name.clone()
                }
            } else {
                name.clone()
            }
        })
        .collect::<Vec<_>>();
    let agents = engine.instruction_files().await;
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let location = std::env::current_dir()
        .ok()
        .map(|path| rho_harness_core::presentation::summary::to_relative_path(&path.display().to_string()))
        .unwrap_or_else(|| ".".to_string());
    let agent_paths = agents
        .iter()
        .map(|path| rho_harness_core::presentation::summary::to_relative_path(path))
        .collect();
    crate::ui::interactive::WelcomeItem {
        version: env!("CARGO_PKG_VERSION").to_string(),
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        location,
        agents: agent_paths,
        tools,
        skills: skill_names,
        mcp,
    }
}

fn print_startup_banner_direct(session: &ReplSession, item: &crate::ui::interactive::WelcomeItem) {
    let renderer = crate::ui::TerminalRenderer {
        theme: crate::ui::theme::detect_with_config(&session.config.ui),
        ..Default::default()
    };
    let width = crate::ui::terminal_width() as usize;
    let rendered = crate::ui::interactive::format_welcome_content(item, width, &renderer.theme);
    let mut stdout = std::io::stdout();
    let trimmed = rendered.trim_end_matches(['\r', '\n']);
    let _ = write!(stdout, "{trimmed}\n\n");
    let _ = stdout.flush();
}

fn build_completions() -> CompletionEngine {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let mut completions = CompletionEngine::new();
    for skill in &skills {
        completions
            .skills
            .push((skill.metadata.name.clone(), skill.metadata.description.clone()));
    }
    for model in &ModelRegistry::default().models {
        completions.models.push((model.id.clone(), model.display_name.clone()));
    }
    completions
}

async fn load_history(session: &ReplSession) -> InteractiveHistory {
    let history_path = session.config.config_dir.join("history.txt");
    InteractiveHistory::with_file_async(1000, history_path)
        .await
        .unwrap_or_else(|_| {
            InteractiveHistory::with_file(1000, std::env::temp_dir().join("rho_fallback_hist.txt"))
                .expect("fallback history")
        })
}

fn format_footer_path(footer: &FooterInfo, width: usize) -> String {
    let left = &footer.path_left;
    let right = footer.quota.as_deref().unwrap_or("");
    let left_w = rho_ui_core::text::visible_width(left);
    let right_w = rho_ui_core::text::visible_width(right);
    let pad = width.saturating_sub(left_w + right_w);
    format!("\x1b[90m{left}{}{right}\x1b[0m", " ".repeat(pad))
}

pub(crate) fn format_footer_stats(footer: &FooterInfo, width: usize) -> String {
    let window = if footer.context_window > 0 {
        footer.context_window
    } else {
        ModelRegistry::default().context_window_for(&footer.model, Some(&footer.provider))
    };
    let metrics = FooterMetrics {
        input_tokens: footer.total_input,
        output_tokens: footer.total_output,
        cache_read_tokens: footer.total_cache_read,
        cache_write_tokens: footer.total_cache_write,
        context_tokens: footer.total_input as usize,
        context_window: window,
        total_cost: None,
        tokens_per_second: footer.tokens_per_second,
        quota_summary: footer.quota.clone(),
    };

    let thinking = footer.thinking.as_deref().unwrap_or("default");
    let right = if thinking == "off" || thinking.is_empty() {
        footer.model.clone()
    } else {
        format!("{} · {thinking}", footer.model)
    };

    let mut parts = Vec::new();
    if metrics.input_tokens > 0 {
        parts.push(format!(
            "↑{}",
            rho_harness_core::tokens::format_tokens(metrics.input_tokens)
        ));
    }
    if metrics.output_tokens > 0 {
        parts.push(format!(
            "↓{}",
            rho_harness_core::tokens::format_tokens(metrics.output_tokens)
        ));
    }
    if metrics.cache_read_tokens > 0 {
        parts.push(format!(
            "R{}",
            rho_harness_core::tokens::format_tokens(metrics.cache_read_tokens)
        ));
    }
    if metrics.cache_write_tokens > 0 {
        parts.push(format!(
            "W{}",
            rho_harness_core::tokens::format_tokens(metrics.cache_write_tokens)
        ));
    }
    if metrics.context_window > 0 {
        let pct = footer.context_percent.unwrap_or_else(|| metrics.context_percent());
        parts.push(format!(
            "{:.1}%/{}",
            pct,
            rho_harness_core::tokens::format_tokens(metrics.context_window as u64)
        ));
    }
    if let Some(tps) = footer.tokens_per_second
        && tps > 0.0
    {
        parts.push(format!("@{tps:.0}t/s"));
    }
    let left = parts.join(" ");

    let left_w = rho_ui_core::text::visible_width(&left);
    let right_w = rho_ui_core::text::visible_width(&right);
    let pad = width.saturating_sub(left_w + right_w);
    format!("\x1b[90m{left}{}{right}\x1b[0m", " ".repeat(pad))
}

fn thinking_divider_style(thinking: Option<&str>) -> (&'static str, &'static str) {
    match thinking.unwrap_or("off") {
        "off" => ("\x1b[38;2;60;60;60m", "\x1b[0m"),
        "minimal" => ("\x1b[90m", "\x1b[0m"),
        "low" => ("\x1b[34m", "\x1b[0m"),
        "medium" => ("\x1b[36m", "\x1b[0m"),
        "high" => ("\x1b[35m", "\x1b[0m"),
        "xhigh" => ("\x1b[31m", "\x1b[0m"),
        "max" => ("\x1b[1;31m", "\x1b[0m"),
        _ => ("\x1b[38;2;60;60;60m", "\x1b[0m"),
    }
}

fn render_live_editor_lines(
    state: &RunnerState<'_>,
    lines: &mut Vec<String>,
    cursor_mode: CursorMode,
) -> (usize, usize) {
    let ed_lines = state.editor.lines();
    let (c_row, c_col) = state.editor.cursor();
    let ed_start = lines.len();
    for (r, ed_line) in ed_lines.iter().enumerate() {
        let mut row = ed_line.clone();
        if r == c_row && cursor_mode == CursorMode::Software {
            apply_software_cursor(&mut row, c_col);
        }
        lines.push(row);
    }
    let c_row = ed_start + c_row.min(ed_lines.len().saturating_sub(1));

    if let Some(popup) = state.autocomplete_popup.as_ref() {
        for (idx, cand) in popup.candidates.iter().take(5).enumerate() {
            let marker = if idx == popup.selected_index { ">" } else { " " };
            let desc = cand.description.as_deref().unwrap_or("");
            lines.push(format!("\x1b[36m {marker} {:<16} {desc}\x1b[0m", cand.display));
        }
    }
    (c_row, c_col)
}

fn build_live_lines(
    state: &RunnerState<'_>,
    footer: &FooterInfo,
    activity: Option<(&Activity, Option<&RunningTool>, usize)>,
    queued_steering: &[String],
    width: usize,
    cursor_mode: CursorMode,
) -> (Vec<String>, usize, usize) {
    let mut lines = Vec::new();

    if let Some((_, Some(tool), _)) = activity {
        let tool_lines = render_running_tool_widget(
            tool,
            &state.session.renderer.theme,
            width,
            state.session.config.ui.tools_expanded.unwrap_or(false),
        );
        let height = crate::ui::terminal_height() as usize;
        let budget = if state.session.config.ui.tools_expanded.unwrap_or(false) {
            ((height as f64) * 0.60).round() as usize
        } else {
            10
        };
        let windowed = window_widget_lines(&tool_lines, budget);
        lines.extend(windowed);
    }

    for steer in queued_steering {
        lines.push(format!("\x1b[36m↳ Steering: {steer}\x1b[0m"));
    }

    let (style, reset) = thinking_divider_style(footer.thinking.as_deref());

    let top_divider = match activity {
        Some((act, _tool, frame)) => {
            let spinner = StreamingSpinner::current_frame(frame);
            let label = if matches!(act, Activity::Compacting) {
                "compacting"
            } else {
                "working"
            };
            let prefix = format!("── {spinner} {label} ");
            let rem = width.saturating_sub(prefix.chars().count());
            format!("{style}{prefix}{}{reset}", "─".repeat(rem))
        }
        None => format!("{style}{}{reset}", "─".repeat(width)),
    };
    lines.push(top_divider);

    let (cursor_row, cursor_col) = if activity.is_some() && state.editor.text().trim().is_empty() {
        lines.push(format!("{style}{}{reset}", "─".repeat(width)));
        (lines.len().saturating_sub(1), 0)
    } else {
        let (c_row, c_col) = render_live_editor_lines(state, &mut lines, cursor_mode);
        lines.push(format!("{style}{}{reset}", "─".repeat(width)));
        (c_row, c_col)
    };

    lines.push(format_footer_path(footer, width));
    lines.push(format_footer_stats(footer, width));

    (lines, cursor_row, cursor_col)
}

struct LiveCursorTarget {
    pub prev_lines: usize,
    pub prev_cursor_row: usize,
    pub target_row: usize,
    pub target_col: usize,
    pub cursor_mode: CursorMode,
    pub active_editor: bool,
}

fn paint_live_region(stdout: &mut std::io::Stdout, lines: &[String], cursor: &LiveCursorTarget) -> std::io::Result<()> {
    if cursor.prev_lines == 0 {
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                stdout.write_all(b"\r\n")?;
            }
            stdout.write_all(line.as_bytes())?;
        }
        let rows_up = lines.len().saturating_sub(1).saturating_sub(cursor.target_row);
        if rows_up > 0 {
            write!(stdout, "\x1b[{rows_up}A")?;
        }
    } else {
        if cursor.prev_cursor_row > 0 {
            write!(stdout, "\x1b[{}A", cursor.prev_cursor_row)?;
        }
        stdout.write_all(b"\r")?;

        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                if i < cursor.prev_lines {
                    stdout.write_all(b"\x1b[1B")?;
                } else {
                    stdout.write_all(b"\r\n")?;
                }
            }
            stdout.write_all(b"\r\x1b[2K")?;
            stdout.write_all(line.as_bytes())?;
        }

        for _ in lines.len()..cursor.prev_lines {
            stdout.write_all(b"\x1b[1B\r\x1b[2K")?;
        }

        let base_height = cursor.prev_lines.max(lines.len());
        let rows_up = base_height.saturating_sub(1).saturating_sub(cursor.target_row);
        if rows_up > 0 {
            write!(stdout, "\x1b[{rows_up}A")?;
        }
    }

    if cursor.target_col > 0 {
        write!(stdout, "\r\x1b[{}C", cursor.target_col)?;
    } else {
        stdout.write_all(b"\r")?;
    }
    if cursor.cursor_mode == CursorMode::Hardware && cursor.active_editor {
        stdout.write_all(b"\x1b[?25h")?;
    } else {
        stdout.write_all(b"\x1b[?25l")?;
    }
    Ok(())
}

fn erase_live_region(stdout: &mut std::io::Stdout, total_lines: usize, cursor_row: usize) -> std::io::Result<()> {
    if total_lines == 0 {
        return Ok(());
    }
    let rows_down = total_lines.saturating_sub(1).saturating_sub(cursor_row);
    if rows_down > 0 {
        write!(stdout, "\x1b[{rows_down}B")?;
    }
    stdout.write_all(b"\r")?;
    for row in (0..total_lines).rev() {
        stdout.write_all(b"\x1b[2K")?;
        if row > 0 {
            stdout.write_all(b"\x1b[1A")?;
        }
    }
    stdout.write_all(b"\r")?;
    stdout.flush()
}

fn refresh_display(
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    activity: Option<(&Activity, Option<&RunningTool>, usize)>,
    queued_steering: &[String],
    extra_output: Option<&str>,
) -> std::io::Result<()> {
    let width = crate::ui::terminal_width() as usize;
    let cursor_mode = state.session.renderer.theme.cursor_mode;
    let (lines, c_row, c_col) = build_live_lines(state, footer, activity, queued_steering, width, cursor_mode);

    let mut stdout = std::io::stdout();
    stdout.write_all(CSI_SYNC_BEGIN)?;

    if let Some(out) = extra_output
        && !out.is_empty()
    {
        erase_live_region(&mut stdout, state.prev_lines_count, state.prev_cursor_row)?;
        state.prev_lines_count = 0;
        state.prev_cursor_row = 0;

        let is_block = out.starts_with('╭')
            || out.starts_with('\n')
            || out.starts_with('\r')
            || out.contains("╭───")
            || out.starts_with("Error:");

        if is_block {
            if state.tracker.is_open() {
                stdout.write_all(b"\r\n")?;
            }
            stdout.write_all(b"\r")?;
            state.tracker.clear();
        } else {
            state.tracker.restore_cursor(&mut stdout, width)?;
        }
        let normalized = terminal_newlines(out);
        stdout.write_all(normalized.as_bytes())?;
        state.tracker.update(&normalized);
        if state.tracker.is_open() {
            stdout.write_all(b"\r\n")?;
        }
    }

    let cursor = LiveCursorTarget {
        prev_lines: state.prev_lines_count,
        prev_cursor_row: state.prev_cursor_row,
        target_row: c_row,
        target_col: c_col,
        cursor_mode,
        active_editor: true,
    };
    paint_live_region(&mut stdout, &lines, &cursor)?;
    stdout.write_all(CSI_SYNC_END)?;
    stdout.flush()?;

    state.prev_lines_count = lines.len();
    state.prev_cursor_row = c_row;
    Ok(())
}

fn full_redraw(
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    activity: Option<(&Activity, Option<&RunningTool>, usize)>,
    queued_steering: &[String],
) -> std::io::Result<()> {
    let width = crate::ui::terminal_width() as usize;
    let mut stdout = std::io::stdout();
    stdout.write_all(CSI_SYNC_BEGIN)?;
    stdout.write_all(b"\x1b[2J\x1b[H")?;
    state.tracker.clear();

    let theme = &state.session.renderer.theme;
    let tools_expanded = state.session.config.ui.tools_expanded.unwrap_or(false);
    let hide_thinking = state.session.config.ui.hide_thinking.unwrap_or(false);

    for item in state.transcript.iter() {
        let input = TranscriptRenderInput {
            item,
            theme,
            width,
            tools_expanded,
            hide_thinking,
        };
        let rendered = crate::ui::interactive::render_transcript_item(input);
        let trimmed = rendered.trim_end_matches(['\r', '\n']);
        if !trimmed.is_empty() {
            let normalized = terminal_newlines(trimmed);
            stdout.write_all(normalized.as_bytes())?;
            stdout.write_all(b"\r\n")?;
            state.tracker.update(&normalized);
        }
    }
    stdout.write_all(b"\r\n")?;

    state.prev_lines_count = 0;
    state.prev_cursor_row = 0;
    let cursor_mode = theme.cursor_mode;
    let (lines, c_row, c_col) = build_live_lines(state, footer, activity, queued_steering, width, cursor_mode);
    let cursor = LiveCursorTarget {
        prev_lines: 0,
        prev_cursor_row: 0,
        target_row: c_row,
        target_col: c_col,
        cursor_mode,
        active_editor: true,
    };
    paint_live_region(&mut stdout, &lines, &cursor)?;
    stdout.write_all(CSI_SYNC_END)?;
    stdout.flush()?;

    state.prev_lines_count = lines.len();
    state.prev_cursor_row = c_row;
    Ok(())
}

fn drain_ui_event(
    ev: UiEvent,
    output: &mut String,
    activity: &mut Activity,
    running_tool: &mut Option<RunningTool>,
    transcript: &mut Vec<TranscriptItem>,
    session: &ReplSession,
) {
    match ev {
        UiEvent::Output(OutputEvent::Text(t) | OutputEvent::StreamText(t)) => {
            output.push_str(&t);
        }
        UiEvent::ToolStart(req) => {
            *running_tool = Some(RunningTool::new(req.name, req.args_summary, req.preview));
        }
        UiEvent::ToolChunk { chunk } => {
            if let Some(tool) = running_tool.as_mut() {
                tool.append_chunk(&chunk);
            }
        }
        UiEvent::ToolEnd => {
            *running_tool = None;
        }
        UiEvent::Transcript(item) => {
            if matches!(item, TranscriptItem::Tool(_)) {
                *running_tool = None;
            }
            transcript.push(item.clone());
            if matches!(item, TranscriptItem::AssistantText(_) | TranscriptItem::Thinking(_)) {
                return;
            }
            let width = crate::ui::terminal_width() as usize;
            let input = TranscriptRenderInput {
                item: &item,
                theme: &session.renderer.theme,
                width,
                tools_expanded: session.config.ui.tools_expanded.unwrap_or(false),
                hide_thinking: session.config.ui.hide_thinking.unwrap_or(false),
            };
            let rendered = crate::ui::interactive::render_transcript_item(input);
            let trimmed = rendered.trim_end_matches(['\r', '\n']);
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(trimmed);
            output.push('\n');
        }
        UiEvent::Activity(act) => {
            *activity = act;
        }
        _ => {}
    }
}

fn handle_popup_input(
    mut popup: AutocompletePopupView,
    key: KeyEvent,
    editor: &mut TextAreaEditor,
) -> (Option<AutocompletePopupView>, bool) {
    match key.code {
        KeyCode::Enter | KeyCode::Tab if key.modifiers.is_empty() => {
            if let Some(cand) = popup.selected() {
                editor.set_text(&cand.value);
            }
            (None, true)
        }
        KeyCode::Esc => (None, true),
        KeyCode::Down => {
            popup.select_next();
            (Some(popup), true)
        }
        KeyCode::Up => {
            popup.select_prev();
            (Some(popup), true)
        }
        _ => (None, false),
    }
}

async fn cycle_model(state: &mut RunnerState<'_>, engine: &mut AgentEngine, direction: i32) {
    let discovered = crate::repl::interactive::discover_models(&state.session.config, &state.session.auth_store);
    if discovered.is_empty() {
        return;
    }
    let current = &state.session.config.model;
    let current_idx = discovered.iter().position(|m| m.id == *current).unwrap_or(0);
    let len = discovered.len() as i32;
    let next_idx = ((current_idx as i32 + direction).rem_euclid(len)) as usize;
    let next = &discovered[next_idx];
    state.session.config.model = next.id.clone();
    state.session.config.provider = next.provider.clone();
    state.session.sync_engine_model(engine).await;
}

async fn cycle_thinking_level(state: &mut RunnerState<'_>, engine: &mut AgentEngine) {
    let current = state.session.config.thinking_level.as_deref().unwrap_or("off");
    let current_idx = THINKING_LEVELS.iter().position(|&l| l == current).unwrap_or(0);
    let next_idx = (current_idx + 1) % THINKING_LEVELS.len();
    let next = THINKING_LEVELS[next_idx];
    state.session.config.thinking_level = Some(next.to_string());
    state.session.sync_engine_model(engine).await;
}

enum ActionOutcome {
    Handled,
    FullRedraw,
    Exit,
}

async fn handle_input_action(
    action: InputAction,
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
) -> Option<ActionOutcome> {
    match action {
        InputAction::Clear => {
            state.editor.clear();
            Some(ActionOutcome::Handled)
        }
        InputAction::Cancel => {
            if !state.editor.is_empty() {
                state.editor.clear();
            }
            Some(ActionOutcome::Handled)
        }
        InputAction::EndOfInput => {
            if state.editor.is_empty() {
                Some(ActionOutcome::Exit)
            } else {
                Some(ActionOutcome::Handled)
            }
        }
        InputAction::ModelSelect => {
            select_model_modal(state.session, engine).await;
            Some(ActionOutcome::FullRedraw)
        }
        InputAction::ModelCycleForward => {
            cycle_model(state, engine, 1).await;
            Some(ActionOutcome::Handled)
        }
        InputAction::ModelCycleBackward => {
            cycle_model(state, engine, -1).await;
            Some(ActionOutcome::Handled)
        }
        InputAction::ThinkingCycle => {
            cycle_thinking_level(state, engine).await;
            Some(ActionOutcome::Handled)
        }
        InputAction::ThinkingToggle => {
            let hide = !state.session.config.ui.hide_thinking.unwrap_or(false);
            state.session.config.ui.hide_thinking = Some(hide);
            Some(ActionOutcome::FullRedraw)
        }
        InputAction::ToggleExpandTools => {
            let exp = !state.session.config.ui.tools_expanded.unwrap_or(false);
            state.session.config.ui.tools_expanded = Some(exp);
            Some(ActionOutcome::FullRedraw)
        }
        #[cfg(unix)]
        InputAction::Suspend => {
            unsafe {
                libc::raise(libc::SIGTSTP);
            }
            Some(ActionOutcome::Handled)
        }
        _ => None,
    }
}

/// The two asynchronous input sources the unified runloop multiplexes:
/// engine-side UI events and terminal input events.
struct LiveInputs<'a> {
    pub ui_events: &'a mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    pub events: &'a mut EventStream,
}

async fn handle_key_cycle(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    key: KeyEvent,
    completions: &CompletionEngine,
    inputs: &mut LiveInputs<'_>,
    footer: &mut FooterInfo,
) -> Result<bool> {
    if let Some(popup) = state.autocomplete_popup.take() {
        let (next_popup, consumed) = handle_popup_input(popup, key, state.editor);
        *state.autocomplete_popup = next_popup;
        if consumed {
            return Ok(false);
        }
    }

    let action = map_key(key);
    if let Some(outcome) = handle_input_action(action, state, engine).await {
        if footer.model != state.session.config.model || footer.thinking != state.session.config.thinking_level {
            *footer = make_footer_info(
                &state.session.config.model,
                &state.session.config.provider,
                state.session.config.thinking_level.as_deref(),
                engine,
            );
        }
        return match outcome {
            ActionOutcome::Exit => Ok(true),
            ActionOutcome::FullRedraw => {
                full_redraw(state, footer, None, &[])?;
                Ok(false)
            }
            ActionOutcome::Handled => Ok(false),
        };
    }

    if key.code == KeyCode::Up && state.editor.cursor().0 == 0 {
        if let Some(prev) = state.history.previous(&state.editor.text()) {
            state.editor.set_text(&prev);
        }
        return Ok(false);
    }

    if key.code == KeyCode::Down && state.editor.cursor().0 == 0 {
        if let Some(next) = state.history.next_entry() {
            state.editor.set_text(&next);
        }
        return Ok(false);
    }

    if !state.editor.handle_key(key) {
        let prompt = state.editor.expanded_text();
        if prompt.trim().is_empty() {
            return Ok(false);
        }
        state.editor.clear();
        let should_exit = handle_submission(state, engine, prompt, inputs).await?;
        *footer = make_footer_info(
            &state.session.config.model,
            &state.session.config.provider,
            state.session.config.thinking_level.as_deref(),
            engine,
        );
        return Ok(should_exit);
    }

    let text = state.editor.text();
    if text.starts_with('/') && !text.contains(' ') {
        let candidates = completions.complete(&text, text.len());
        *state.autocomplete_popup = (!candidates.is_empty()).then(|| AutocompletePopupView::new(candidates));
    } else {
        *state.autocomplete_popup = None;
    }

    Ok(false)
}

struct LiveContext {
    pub engine: AgentEngine,
    pub ui_events: tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    pub editor: TextAreaEditor,
    pub history: InteractiveHistory,
    pub completions: CompletionEngine,
    pub transcript: Vec<TranscriptItem>,
}

async fn init_live_context(session: &mut ReplSession) -> Result<LiveContext> {
    let engine = init_live_engine(session).await?;
    let welcome = build_welcome_item(session, &engine).await;
    print_startup_banner_direct(session, &welcome);

    let (ui, ui_events) = InteractiveUi::channel();
    session.renderer = crate::ui::TerminalRenderer::with_ui(ui);
    session.renderer.theme = crate::ui::theme::detect_with_config(&session.config.ui);
    session.renderer.set_width(crate::ui::terminal_width() as usize);

    let mode = if session.config.editor.is_vim() {
        EditorMode::Vim
    } else {
        EditorMode::Default
    };
    let editor = TextAreaEditor::new(mode);
    let history = load_history(session).await;
    crate::repl::interactive::spawn_background_model_refresh(&session.config, &session.auth_store);
    let completions = build_completions();
    let transcript = vec![TranscriptItem::Welcome(welcome)];

    Ok(LiveContext {
        engine,
        ui_events,
        editor,
        history,
        completions,
        transcript,
    })
}

fn handle_live_resize(width: usize, state: &mut RunnerState<'_>, engine: &AgentEngine) -> Result<()> {
    state.session.renderer.set_width(width);
    let footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    full_redraw(state, &footer, None, &[])?;
    Ok(())
}

fn handle_live_paste(text: &str, state: &mut RunnerState<'_>, engine: &AgentEngine) -> Result<()> {
    state.editor.handle_paste(text);
    let footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    refresh_display(state, &footer, None, &[], None)?;
    Ok(())
}

pub async fn run_unified_live(session: &mut ReplSession) -> Result<()> {
    let mut ctx = init_live_context(session).await?;
    let _guard = TerminalGuard::enter()?;

    let mut autocomplete_popup: Option<AutocompletePopupView> = None;
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    let mut tracker = OutputTracker::default();

    let mut state = RunnerState {
        session,
        editor: &mut ctx.editor,
        history: &mut ctx.history,
        autocomplete_popup: &mut autocomplete_popup,
        transcript: &mut ctx.transcript,
        tracker: &mut tracker,
        prev_lines_count: 0,
        prev_cursor_row: 0,
    };

    let mut footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        &ctx.engine,
    );
    refresh_display(&mut state, &footer, None, &[], None)?;

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else {
                    break;
                };

                match event {
                    Event::Resize(w, _) => {
                        handle_live_resize((w as usize).max(1), &mut state, &ctx.engine)?;
                    }
                    Event::Paste(text) => {
                        handle_live_paste(&text, &mut state, &ctx.engine)?;
                    }
                    Event::Key(key) => {
                        if key.kind == crossterm::event::KeyEventKind::Release {
                            continue;
                        }
                        let mut inputs = LiveInputs {
                            ui_events: &mut ctx.ui_events,
                            events: &mut events,
                        };
                        let should_exit = handle_key_cycle(
                            &mut state,
                            &mut ctx.engine,
                            key,
                            &ctx.completions,
                            &mut inputs,
                            &mut footer,
                        ).await?;
                        if should_exit {
                            break;
                        }
                        refresh_display(&mut state, &footer, None, &[], None)?;
                    }
                    _ => {}
                }
            }
        }
    }

    let mut stdout = std::io::stdout();
    erase_live_region(&mut stdout, state.prev_lines_count, state.prev_cursor_row)?;
    Ok(())
}

fn model_registry_for(session: &ReplSession) -> ModelRegistry {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let models = discovered
        .into_iter()
        .map(|item| ModelCapability {
            context_tokens: ModelRegistry::resolve_context_window(&item.id, Some(&item.provider)),
            supports_reasoning: item.description.to_ascii_lowercase().contains("reasoning"),
            is_local: item.provider.eq_ignore_ascii_case("local") || item.provider.eq_ignore_ascii_case("ollama"),
            display_name: item.description,
            id: item.id,
            provider: item.provider,
        })
        .collect();
    ModelRegistry::new(models, session.config.model.clone())
}

fn settings_state_for(session: &ReplSession) -> SettingsState {
    SettingsState {
        hide_thinking: session.config.ui.hide_thinking.unwrap_or(false),
        tools_expanded: session.config.ui.tools_expanded.unwrap_or(true),
        vim_mode: session.config.editor.is_vim(),
        show_version_banner: session.config.show_label,
    }
}

fn mcp_modal_state_for(session: &ReplSession) -> McpModalState {
    let statuses = rho_engine::mcp::get_mcp_server_statuses();
    let servers = session
        .config
        .mcp
        .servers
        .iter()
        .map(|(name, cfg)| {
            let (status_message, enabled, tools_count) = match statuses.get(name) {
                Some(st) if st.error.is_some() => (
                    format!("failed: {}", st.error.clone().unwrap_or_default()),
                    false,
                    st.tools_count,
                ),
                Some(st) if st.is_loaded => ("active".to_string(), true, st.tools_count),
                Some(st) => ("disabled".to_string(), false, st.tools_count),
                None if cfg.enabled => ("active".to_string(), true, 0),
                None => ("disabled".to_string(), false, 0),
            };
            McpServerInfo {
                name: name.clone(),
                command: cfg.command.clone().or_else(|| cfg.url.clone()).unwrap_or_default(),
                enabled,
                status_message,
                tools_count,
            }
        })
        .collect();
    McpModalState::new(servers)
}

fn skill_modal_state() -> SkillModalState {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref())
        .into_iter()
        .map(|s| SkillInfo {
            name: s.metadata.name,
            description: s.metadata.description,
            origin: format!("{:?}", s.origin),
            path: Some(s.metadata.location),
        })
        .collect();
    SkillModalState::new(skills)
}

/// Runs a Ratatui modal to completion and returns the confirmed selection value.
/// Clears the inline live region so a modal owns the rows beneath the
/// transcript, and resets diff bookkeeping so the next paint starts fresh.
fn suspend_live_region(state: &mut RunnerState<'_>) {
    let mut stdout = std::io::stdout();
    let _ = erase_live_region(&mut stdout, state.prev_lines_count, state.prev_cursor_row);
    state.prev_lines_count = 0;
    state.prev_cursor_row = 0;
}

fn is_modal_command(cmd: &str) -> bool {
    matches!(cmd, "/model" | "/thinking" | "/settings" | "/session" | "/mcp" | "/skill")
}

fn prompt_modal_selection(view: &mut StandardModalView) -> Option<String> {
    if run_modal_view(view).ok()? {
        view.state.selected_option().map(|opt| opt.value.clone())
    } else {
        None
    }
}

async fn apply_model_selection(session: &mut ReplSession, engine: &mut AgentEngine, selected: &str) {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    if let Some(item) = discovered.iter().find(|d| d.id == selected) {
        session.config.model = item.id.clone();
        session.config.provider = item.provider.clone();
    } else {
        session.config.model = selected.to_string();
    }
    session.sync_engine_model(engine).await;
}

async fn select_model_modal(session: &mut ReplSession, engine: &mut AgentEngine) {
    let registry = model_registry_for(session);
    let mut view = StandardModalView::model(&registry);
    if let Some(selected) = prompt_modal_selection(&mut view) {
        apply_model_selection(session, engine, &selected).await;
    }
}

fn apply_settings_selection(session: &mut ReplSession, selected: &str) {
    match selected {
        "toggle_thinking" => {
            let hide = !session.config.ui.hide_thinking.unwrap_or(false);
            session.config.ui.hide_thinking = Some(hide);
        }
        "toggle_tools" => {
            let expanded = !session.config.ui.tools_expanded.unwrap_or(true);
            session.config.ui.tools_expanded = Some(expanded);
        }
        "toggle_vim" => {
            let vim = !session.config.editor.is_vim();
            session.config.editor.mode = Some(if vim { "vim".to_string() } else { "default".to_string() });
        }
        "toggle_banner" => {
            session.config.show_label = !session.config.show_label;
        }
        _ => {}
    }
}

async fn handle_interactive_command(cmd: &str, session: &mut ReplSession, engine: &mut AgentEngine) -> bool {
    match cmd {
        "/model" => {
            select_model_modal(session, engine).await;
            true
        }
        "/thinking" => {
            let mut view = StandardModalView::thinking(session.config.thinking_level.as_deref());
            if let Some(level) = prompt_modal_selection(&mut view) {
                session.config.thinking_level = Some(level);
                session.sync_engine_model(engine).await;
            }
            true
        }
        "/settings" => {
            let settings = settings_state_for(session);
            let mut view = StandardModalView::settings(&settings);
            if let Some(selected) = prompt_modal_selection(&mut view) {
                apply_settings_selection(session, &selected);
            }
            true
        }
        "/session" => {
            let summaries = list_session_summaries_async(&session.config.sessions_dir)
                .await
                .unwrap_or_default();
            let mut view = StandardModalView::session(&summaries, Some(&engine.session_manager.session_id));
            if let Some(id) = prompt_modal_selection(&mut view) {
                session.resume_id = Some(id);
            }
            true
        }
        "/mcp" => {
            let mcp_state = mcp_modal_state_for(session);
            let mut view = StandardModalView::mcp(&mcp_state);
            let _ = prompt_modal_selection(&mut view);
            true
        }
        "/skill" => {
            let skill_state = skill_modal_state();
            let mut view = StandardModalView::skill(&skill_state);
            let _ = prompt_modal_selection(&mut view);
            true
        }
        _ => false,
    }
}

/// Projects an engine-issued interaction prompt into the shared modal state so
/// tool approvals render through the same Ratatui modal as every other selector.
fn interaction_modal_view(prompt: &InteractionPrompt) -> StandardModalView {
    let options = prompt
        .options
        .iter()
        .enumerate()
        .map(|(idx, opt)| ModalOption {
            label: format!("{:<16}", opt.label),
            description: opt.description.clone(),
            value: idx.to_string(),
            is_active: idx == prompt.initial_selection,
            shortcut: None,
        })
        .collect();
    let mut state = ModalState::new(prompt.title.clone(), options).with_search(false);
    state.subtitle = prompt.body.clone();
    state.selected_index = prompt
        .initial_selection
        .min(state.filtered_options.len().saturating_sub(1));
    let inline_inputs = prompt
        .options
        .iter()
        .enumerate()
        .filter_map(|(idx, opt)| opt.input.clone().map(|spec| (idx.to_string(), spec)));
    StandardModalView::new(state).with_inline_inputs(inline_inputs)
}

fn resolve_interaction(prompt: &InteractionPrompt, responder: InteractionResponder) {
    let mut view = interaction_modal_view(prompt);
    let response = if run_modal_view(&mut view).unwrap_or(false) {
        interaction_response_for(&view)
    } else {
        InteractionResponse::Cancelled
    };
    let _ = responder.respond(response);
}

fn interaction_response_for(view: &StandardModalView) -> InteractionResponse {
    if let Some((option_value, text)) = view.submitted_input() {
        return match option_value.parse::<usize>() {
            Ok(index) => InteractionResponse::SelectedWithInput {
                index,
                text: text.to_string(),
            },
            Err(_) => InteractionResponse::Cancelled,
        };
    }
    view.state
        .selected_option()
        .and_then(|opt| opt.value.parse::<usize>().ok())
        .map(InteractionResponse::Selected)
        .unwrap_or(InteractionResponse::Cancelled)
}

async fn handle_slash_command(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    cmd: &str,
    rest: &str,
) -> Result<bool> {
    if matches!(cmd, "/quit" | "/exit") {
        return Ok(true);
    }
    if cmd == "/clear" {
        let mut stdout = std::io::stdout();
        stdout.write_all(b"\x1b[2J\x1b[H")?;
        stdout.flush()?;
        return Ok(false);
    }
    if rest.is_empty() && is_modal_command(cmd) {
        suspend_live_region(state);
        if handle_interactive_command(cmd, state.session, engine).await {
            return Ok(false);
        }
    }
    if cmd == "/model" && !rest.is_empty() {
        let discovered = crate::repl::interactive::discover_models(&state.session.config, &state.session.auth_store);
        if let Some(m) = discovered.iter().find(|d| d.id == rest) {
            state.session.config.model = m.id.clone();
            state.session.config.provider = m.provider.clone();
        } else {
            state.session.config.model = rest.to_string();
        }
        state.session.sync_engine_model(engine).await;
        return Ok(false);
    }
    if cmd == "/thinking" && !rest.is_empty() {
        state.session.config.thinking_level = Some(rest.to_string());
        state.session.sync_engine_model(engine).await;
        return Ok(false);
    }

    let mut cmd_ctx = SlashCommandContext {
        config: &mut state.session.config,
        auth_store: &mut state.session.auth_store,
        renderer: &state.session.renderer,
        session_id: Some(&engine.session_manager.session_id),
        session_manager: Some(&engine.session_manager),
        engine: Some(engine),
        home_dir: None,
    };
    let trimmed = if rest.is_empty() {
        cmd.to_string()
    } else {
        format!("{cmd} {rest}")
    };
    let should_exit = matches!(
        SlashCommandHandler::handle(&trimmed, &mut cmd_ctx).await,
        Ok(Some(CommandResult::Exit))
    );
    Ok(should_exit)
}

fn broadcast_turn_start(prompt: &str) {
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::TurnStart {
        turn_number: 1,
        prompt: prompt.to_string(),
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::StatusChanged {
        status: "busy".to_string(),
    });
}

fn broadcast_turn_completion(engine: &AgentEngine) {
    let totals = engine.session_usage_totals();
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::UsageUpdate {
        input_tokens: Some(totals.total_input),
        output_tokens: Some(totals.total_output),
        cache_read_tokens: Some(totals.total_cache_read),
        cache_write_tokens: Some(totals.total_cache_write),
        total_cost: None,
        context_percent: None,
        context_window: None,
        tokens_per_second: None,
        quota: None,
    });
    crate::platform::remote::PEER_REGISTRY.broadcast(&RpcEvent::StatusChanged {
        status: "idle".to_string(),
    });
}

fn print_initial_prompt(state: &mut RunnerState<'_>, prompt: &str, footer: &FooterInfo) -> Result<()> {
    let width = crate::ui::terminal_width() as usize;
    let user_box = state.session.renderer.theme.user_block(width).render_plain(prompt);
    let trimmed = user_box.trim_end_matches(['\r', '\n']);
    let pending_scrollback = format!("{trimmed}\n");
    refresh_display(state, footer, None, &[], Some(&pending_scrollback))?;
    Ok(())
}

async fn finish_turn_execution(state: &mut RunnerState<'_>, engine: &mut AgentEngine) -> Result<()> {
    state.session.sync_engine_model(engine).await;
    engine.refresh_quota().await;
    broadcast_turn_completion(engine);

    state.tracker.clear();
    let updated_footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    refresh_display(state, &updated_footer, None, &[], None)?;
    Ok(())
}

enum TurnKeyOutcome {
    Handled,
    FullRedraw,
    Cancel,
}

fn handle_turn_key_input(
    key: KeyEvent,
    state: &mut RunnerState<'_>,
    steering: &SharedSteeringQueue,
    cancellation: &CancellationSignal,
) -> TurnKeyOutcome {
    let action = map_key(key);
    match action {
        InputAction::Cancel => {
            cancellation.cancel();
            rho_engine::process::kill_all_tracked_processes();
            TurnKeyOutcome::Cancel
        }
        InputAction::Clear => {
            state.editor.clear();
            TurnKeyOutcome::Handled
        }
        InputAction::ToggleExpandTools => {
            let exp = !state.session.config.ui.tools_expanded.unwrap_or(false);
            state.session.config.ui.tools_expanded = Some(exp);
            TurnKeyOutcome::FullRedraw
        }
        InputAction::ThinkingToggle => {
            let hide = !state.session.config.ui.hide_thinking.unwrap_or(false);
            state.session.config.ui.hide_thinking = Some(hide);
            TurnKeyOutcome::FullRedraw
        }
        _ => {
            if (key.code == KeyCode::Up || (key.code == KeyCode::Up && key.modifiers.contains(KeyModifiers::ALT)))
                && state.editor.is_empty()
                && let Some(popped) = steering.pop_last()
            {
                state.editor.set_text(&popped);
                return TurnKeyOutcome::Handled;
            }
            if !state.editor.handle_key(key) {
                let steering_text = state.editor.expanded_text();
                let trimmed = steering_text.trim().to_string();
                if !trimmed.is_empty() {
                    steering.enqueue(trimmed);
                    rho_engine::process::kill_all_tracked_processes();
                    state.editor.clear();
                }
            }
            TurnKeyOutcome::Handled
        }
    }
}

fn init_turn_channels(
    engine: &AgentEngine,
    renderer: crate::ui::TerminalRenderer,
) -> (
    Arc<SharedSteeringQueue>,
    Arc<dyn rho_harness_core::presentation::Presenter>,
) {
    let steering = Arc::new(SharedSteeringQueue::new(engine.config.steering_mode));
    crate::platform::remote::set_active_steering(Some(steering.clone()));
    let broadcast: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(
        crate::ui::render::BroadcastPresenter::new(Arc::new(renderer), crate::platform::remote::PEER_REGISTRY.clone()),
    );
    (steering, broadcast)
}

fn flush_or_refresh_turn_tick(
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    activity_meta: (&Activity, Option<&RunningTool>, usize),
    queued_steering: &[String],
    pending_scrollback: &mut String,
) -> Result<()> {
    if !pending_scrollback.is_empty() {
        let out = std::mem::take(pending_scrollback);
        refresh_display(state, footer, Some(activity_meta), queued_steering, Some(&out))?;
    } else {
        refresh_display(state, footer, Some(activity_meta), queued_steering, None)?;
    }
    Ok(())
}

#[derive(Default)]
struct TurnStreamState {
    pub activity: Activity,
    pub running_tool: Option<RunningTool>,
    pub scrollback: String,
    pub tick_counter: usize,
    pub spinner_frame: usize,
}

fn handle_turn_event(ui_ev: UiEvent, state: &mut RunnerState<'_>, stream: &mut TurnStreamState) {
    match ui_ev {
        UiEvent::Interaction { prompt: p, responder } => {
            suspend_live_region(state);
            resolve_interaction(&p, responder);
        }
        other => {
            drain_ui_event(
                other,
                &mut stream.scrollback,
                &mut stream.activity,
                &mut stream.running_tool,
                state.transcript,
                state.session,
            );
        }
    }
}

fn handle_turn_resize(
    width: usize,
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &TurnStreamState,
    steering: &SharedSteeringQueue,
) -> Result<()> {
    state.session.renderer.set_width(width);
    let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
    let queued = steering.current_items();
    full_redraw(state, footer, Some(activity_meta), &queued)?;
    Ok(())
}

fn finalize_turn_result<T>(
    res: Result<T>,
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &mut TurnStreamState,
) -> Result<()> {
    if let Err(ref err) = res {
        stream.scrollback.push_str(&format!("Error: {err}\n"));
    }
    let trimmed = stream.scrollback.trim_end_matches(['\r', '\n']);
    let out = if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    };
    refresh_display(state, footer, None, &[], if out.is_empty() { None } else { Some(&out) })?;
    stream.scrollback.clear();
    Ok(())
}

fn sync_footer_usage(footer: &mut FooterInfo, usage: &rho_engine::engine::tracking::UsageTracker) {
    let totals = usage.totals();
    footer.total_input = totals.total_input;
    footer.total_output = totals.total_output;
    footer.total_cache_read = totals.total_cache_read;
    footer.total_cache_write = totals.total_cache_write;
    footer.tokens_per_second = usage.tokens_per_second();
    if footer.context_window > 0 {
        let active_tokens = usage
            .latest()
            .map(|latest| rho_engine::engine::consumed_context_tokens(&latest, &footer.provider))
            .unwrap_or(totals.total_input);
        footer.context_percent =
            Some(((active_tokens as f64 / footer.context_window as f64) * 100.0).clamp(0.0, 100.0));
    }
}

fn handle_turn_tick(
    state: &mut RunnerState<'_>,
    usage: &rho_engine::engine::tracking::UsageTracker,
    footer: &mut FooterInfo,
    stream: &mut TurnStreamState,
    steering: &SharedSteeringQueue,
) -> Result<()> {
    stream.tick_counter += 1;
    if stream.tick_counter.is_multiple_of(5) {
        stream.spinner_frame = (stream.spinner_frame + 1) % 10;
        sync_footer_usage(footer, usage);
    }
    let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
    let queued = steering.current_items();
    flush_or_refresh_turn_tick(state, footer, activity_meta, &queued, &mut stream.scrollback)
}

fn handle_turn_paste(
    text: &str,
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &TurnStreamState,
    steering: &SharedSteeringQueue,
) -> Result<()> {
    state.editor.handle_paste(text);
    let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
    let queued = steering.current_items();
    refresh_display(state, footer, Some(activity_meta), &queued, None)?;
    Ok(())
}

fn handle_turn_key(
    key: KeyEvent,
    state: &mut RunnerState<'_>,
    steering: &SharedSteeringQueue,
    cancellation: &CancellationSignal,
    stream: &mut TurnStreamState,
    footer: &FooterInfo,
) -> Result<bool> {
    let outcome = handle_turn_key_input(key, state, steering, cancellation);
    match outcome {
        TurnKeyOutcome::Cancel => Ok(true),
        TurnKeyOutcome::FullRedraw => {
            let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
            let queued = steering.current_items();
            full_redraw(state, footer, Some(activity_meta), &queued)?;
            Ok(false)
        }
        TurnKeyOutcome::Handled => {
            let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
            let queued = steering.current_items();
            let _ = refresh_display(state, footer, Some(activity_meta), &queued, None);
            Ok(false)
        }
    }
}

async fn finalize_turn_cancellation(
    engine: &mut AgentEngine,
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &mut TurnStreamState,
) -> Result<()> {
    let _ = engine.record_cancellation("operator interrupt").await;
    refresh_display(state, footer, None, &[], Some("\nCanceled.\n"))?;
    stream.scrollback.clear();
    Ok(())
}

async fn dispatch_unconsumed_steering(
    unconsumed: Vec<String>,
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    inputs: &mut LiveInputs<'_>,
) -> Result<()> {
    for msg in unconsumed {
        Box::pin(execute_agent_turn(state, engine, &msg, inputs)).await?;
    }
    Ok(())
}

fn handle_turn_stream_event(
    ev: Option<std::io::Result<Event>>,
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &mut TurnStreamState,
    steering: &SharedSteeringQueue,
    cancellation: &CancellationSignal,
) -> Result<bool> {
    match ev {
        Some(Ok(Event::Resize(w, _))) => {
            handle_turn_resize((w as usize).max(1), state, footer, stream, steering)?;
        }
        Some(Ok(Event::Paste(text))) => {
            handle_turn_paste(&text, state, footer, stream, steering)?;
        }
        Some(Ok(Event::Key(key))) => {
            return handle_turn_key(key, state, steering, cancellation, stream, footer);
        }
        _ => {}
    }
    Ok(false)
}

async fn execute_agent_turn(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    prompt: &str,
    inputs: &mut LiveInputs<'_>,
) -> Result<()> {
    while inputs.ui_events.try_recv().is_ok() {}

    state.transcript.push(TranscriptItem::UserMessage(prompt.to_string()));
    state.tracker.clear();

    let usage = engine.usage().clone();
    let mut footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    print_initial_prompt(state, prompt, &footer)?;

    let cancellation = Arc::new(CancellationSignal::default());
    let (steering, broadcast) = init_turn_channels(engine, state.session.renderer.clone());
    let request = TurnRequest::new(prompt)
        .with_cancellation(&cancellation)
        .with_steering(steering.clone());
    broadcast_turn_start(prompt);

    let mut turn_future = Box::pin(engine.run_turn(request, broadcast));
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    let mut stream = TurnStreamState::default();
    let mut is_cancelled = false;

    loop {
        tokio::select! {
            res = &mut turn_future => {
                while let Ok(ui_ev) = inputs.ui_events.try_recv() {
                    drain_ui_event(ui_ev, &mut stream.scrollback, &mut stream.activity, &mut stream.running_tool, state.transcript, state.session);
                }
                sync_footer_usage(&mut footer, &usage);
                finalize_turn_result(res, state, &footer, &mut stream)?;
                break;
            }
            Some(ui_ev) = inputs.ui_events.recv() => {
                handle_turn_event(ui_ev, state, &mut stream);
            }
            _ = ticker.tick() => {
                handle_turn_tick(state, &usage, &mut footer, &mut stream, &steering)?;
            }
            maybe_key = inputs.events.next() => {
                if handle_turn_stream_event(maybe_key, state, &footer, &mut stream, &steering, &cancellation)? {
                    is_cancelled = true;
                    break;
                }
            }
        }
    }
    drop(turn_future);
    if is_cancelled {
        finalize_turn_cancellation(engine, state, &footer, &mut stream).await?;
    }
    crate::platform::remote::set_active_steering(None);

    finish_turn_execution(state, engine).await?;
    let unconsumed = steering.current_items();
    if !unconsumed.is_empty() && !is_cancelled {
        steering.clear();
        dispatch_unconsumed_steering(unconsumed, state, engine, inputs).await?;
    }
    Ok(())
}

async fn handle_submission(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    prompt: String,
    inputs: &mut LiveInputs<'_>,
) -> Result<bool> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    let _ = state.history.record(trimmed);

    if trimmed.starts_with('/') {
        let (cmd, rest) = match trimmed.split_once(' ') {
            Some((c, r)) => (c, r.trim()),
            None => (trimmed, ""),
        };
        return handle_slash_command(state, engine, cmd, rest).await;
    }

    execute_agent_turn(state, engine, trimmed, inputs).await?;
    Ok(false)
}
