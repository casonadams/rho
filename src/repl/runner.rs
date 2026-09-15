use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use rho_harness_core::presentation::{InteractionPrompt, WelcomeDisplay};
use rho_harness_core::rpc::protocol::RpcEvent;
use rho_harness_core::session::list_session_summaries_async;
use rho_ui_core::autocomplete::CompletionEngine;
use rho_ui_core::keymap::{InputAction, map_key};
use rho_ui_core::modal::{
    McpModalState, McpServerInfo, ModelCapability, ModelRegistry, SettingsState, SkillInfo, SkillModalState,
};
use rho_ui_core::session::PROVIDER_DEFS;
use rho_ui_core::state::{FooterMetrics, RhoTicket};

use crate::engine::AgentEngine;
use crate::engine::runner::{CancellationSignal, TurnRequest};
use crate::error::Result;
use crate::repl::ReplSession;
use crate::repl::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use crate::repl::coordinator::SharedSteeringQueue;
use crate::repl::interactive::InteractiveHistory;
use crate::ui::editor::{EditorMode, TextAreaEditor};
use crate::ui::interactive::{
    Activity, InteractionResponder, InteractionResponse, InteractiveUi, OutputEvent, RunningTool,
    RunningToolWidgetInput, TranscriptItem, TranscriptRenderInput, UiEvent, render_running_tool_widget,
};
use crate::ui::modal::{AutocompletePopupView, RemotePairModalView, StandardModalView};
use crate::ui::terminal::TerminalGuard;
use crate::ui::theme::CursorMode;
use crate::ui::widgets::StreamingSpinner;
use crate::ui::{ModalView, PromptEditor};

const CSI_SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const CSI_SYNC_END: &[u8] = b"\x1b[?2026l";
const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

struct PermissionModal {
    pub prompt: InteractionPrompt,
    pub selected_index: usize,
    pub body_scroll: usize,
    pub is_editing: bool,
    pub editor: TextAreaEditor,
    pub custom_deny_reason: String,
    pub resolved: Option<InteractionResponse>,
}

impl PermissionModal {
    fn new(prompt: InteractionPrompt) -> Self {
        let default_cmd = prompt
            .options
            .get(1)
            .and_then(|o| o.input.as_ref())
            .and_then(|i| i.value.clone())
            .unwrap_or_else(|| {
                if let Some(line) = prompt.body.lines().find(|l| l.starts_with("Input: ")) {
                    line.trim_start_matches("Input: ").to_string()
                } else {
                    prompt.body.clone()
                }
            });
        let mut editor = TextAreaEditor::new(EditorMode::Default);
        editor.set_text(&default_cmd);
        let selected_index = prompt.initial_selection.min(prompt.options.len().saturating_sub(1));
        Self {
            prompt,
            selected_index,
            body_scroll: 0,
            is_editing: false,
            editor,
            custom_deny_reason: String::new(),
            resolved: None,
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                self.is_editing = false;
                self.resolved = Some(InteractionResponse::SelectedWithInput {
                    index: 1,
                    text: self.editor.text().to_string(),
                });
                true
            }
            KeyCode::Esc => {
                self.is_editing = false;
                true
            }
            _ => self.editor.handle_key(key),
        }
    }

    fn handle_selection_key(&mut self, key: KeyEvent) -> bool {
        let n_opts = self.prompt.options.len().max(1);
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => {
                self.resolved = Some(InteractionResponse::Cancelled);
                true
            }
            (KeyCode::Left, _) | (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::BackTab, _) => {
                if self.selected_index == 0 {
                    self.selected_index = n_opts - 1;
                } else {
                    self.selected_index -= 1;
                }
                true
            }
            (KeyCode::Right, _) | (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
                self.selected_index = (self.selected_index + 1) % n_opts;
                true
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.body_scroll = self.body_scroll.saturating_sub(1);
                true
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.body_scroll += 1;
                true
            }
            (KeyCode::Char(c), KeyModifiers::NONE) if c >= '1' && (c as usize - '1' as usize) < n_opts => {
                self.selected_index = c as usize - '1' as usize;
                if self.selected_index == 1 {
                    self.is_editing = true;
                }
                true
            }
            (KeyCode::Enter, _) => {
                match self.selected_index {
                    0 => self.resolved = Some(InteractionResponse::Selected(0)),
                    1 => self.is_editing = true,
                    2 => self.resolved = Some(InteractionResponse::Selected(2)),
                    3 => {
                        let text = self.custom_deny_reason.trim().to_string();
                        self.resolved = if text.is_empty() {
                            Some(InteractionResponse::Cancelled)
                        } else {
                            Some(InteractionResponse::SelectedWithInput { index: 3, text })
                        };
                    }
                    other => self.resolved = Some(InteractionResponse::Selected(other)),
                }
                true
            }
            (KeyCode::Backspace, _) if self.selected_index == 3 => {
                self.custom_deny_reason.pop();
                true
            }
            (KeyCode::Char(c), KeyModifiers::NONE) if self.selected_index == 3 => {
                self.custom_deny_reason.push(c);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.is_editing {
            self.handle_editing_key(key)
        } else {
            self.handle_selection_key(key)
        }
    }

    fn render_lines(&self, width: usize, cursor_mode: CursorMode) -> (Vec<String>, usize, usize) {
        let mut lines = Vec::new();
        if self.is_editing {
            let sep = "─".repeat(width.saturating_sub(38));
            lines.push(format!("\x1b[33m── Permission Required · Edit Command {sep}\x1b[0m"));
            lines.push("\x1b[90mModify tool arguments before running:\x1b[0m".to_string());
            lines.push(String::new());
            let mut ed_line = format!("  {}", self.editor.text());
            let (_, c_col) = self.editor.cursor();
            let cursor_col = c_col + 2;
            if cursor_mode == CursorMode::Software {
                crate::ui::interactive::apply_software_cursor(&mut ed_line, cursor_col);
            }
            lines.push(ed_line);
            let c_row = lines.len() - 1;
            lines.push(String::new());
            lines.push("\x1b[90m[Enter] Confirm Edit  ·  [Esc] Cancel\x1b[0m".to_string());
            lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
            (lines, c_row, cursor_col)
        } else {
            let sep = "─".repeat(width.saturating_sub(26));
            lines.push(format!("\x1b[33m── Permission Required {sep}\x1b[0m"));
            let body_lines: Vec<&str> = self.prompt.body.lines().collect();
            let max_body_lines = 6;
            let start = self.body_scroll.min(body_lines.len().saturating_sub(1));
            for line in body_lines.iter().skip(start).take(max_body_lines) {
                lines.push(format!("  {line}"));
            }
            if body_lines.len() > max_body_lines {
                let current = start + 1;
                let total = body_lines.len();
                lines.push(format!("\x1b[90m  ↑/↓ scroll body (line {current}/{total})\x1b[0m"));
            }

            lines.push(String::new());
            let mut action_spans = Vec::new();
            for (i, opt) in self.prompt.options.iter().enumerate() {
                let digit = i + 1;
                let is_sel = i == self.selected_index;
                if is_sel {
                    action_spans.push(format!("\x1b[1;33m[{digit}. {}]\x1b[0m", opt.label));
                } else {
                    action_spans.push(format!("\x1b[90m{digit}. {}\x1b[0m", opt.label));
                }
            }
            lines.push(format!("  {}", action_spans.join("   ")));

            if self.selected_index == 3 {
                let reason = &self.custom_deny_reason;
                lines.push(format!("  \x1b[31mDenial reason: {reason}█\x1b[0m"));
            } else {
                lines.push("\x1b[90m  ←/→ select · 1-4 jump · Enter confirm · Esc deny\x1b[0m".to_string());
            }
            lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
            let c_row = lines.len() - 1;
            (lines, c_row, 0)
        }
    }
}

enum ActiveModal {
    Standard(StandardModalView),
    RemotePair(RemotePairModalView),
    Permission(Box<PermissionModal>),
}

impl ActiveModal {
    fn is_open(&self) -> bool {
        match self {
            Self::Standard(m) => m.state.is_open,
            Self::RemotePair(m) => m.is_open,
            Self::Permission(p) => p.resolved.is_none(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self {
            Self::Standard(m) => m.handle_key(key),
            Self::RemotePair(m) => m.handle_key(key),
            Self::Permission(p) => p.handle_key(key),
        }
    }

    fn render_lines(&self, width: usize, cursor_mode: CursorMode) -> (Vec<String>, usize, usize) {
        match self {
            Self::Standard(m) => {
                let mut lines = Vec::new();
                let title = &m.state.title;
                let query = &m.state.filter_query;
                let header = if query.is_empty() {
                    format!("── {title} ")
                } else {
                    format!("── {title} (filter: {query}) ")
                };
                let sep = "─".repeat(width.saturating_sub(header.chars().count()));
                lines.push(format!("\x1b[36m{header}{sep}\x1b[0m"));
                let total = m.state.filtered_options.len();
                if total == 0 {
                    lines.push("\x1b[90m  No matching options\x1b[0m".to_string());
                } else {
                    let max_visible = 8;
                    let window_start = if m.state.selected_index < max_visible {
                        0
                    } else {
                        m.state.selected_index.saturating_sub(max_visible - 1)
                    };
                    for (window_idx, opt) in m
                        .state
                        .filtered_options
                        .iter()
                        .skip(window_start)
                        .take(max_visible)
                        .enumerate()
                    {
                        let actual_idx = window_start + window_idx;
                        let marker = if actual_idx == m.state.selected_index { ">" } else { " " };
                        let active = if opt.is_active { " ✓" } else { "" };
                        let desc = opt.description.as_deref().unwrap_or("");
                        let line = format!(" {marker} {:<18} {desc}{active}", opt.label);
                        if actual_idx == m.state.selected_index {
                            lines.push(format!("\x1b[1;36m{line}\x1b[0m"));
                        } else {
                            lines.push(format!("\x1b[90m{line}\x1b[0m"));
                        }
                    }
                    if total > max_visible {
                        let cur = m.state.selected_index + 1;
                        lines.push(format!("\x1b[90m  ↑/↓ scroll ({cur}/{total})\x1b[0m"));
                    }
                }
                lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
                let len = lines.len();
                (lines, len.saturating_sub(1), 0)
            }
            Self::RemotePair(m) => {
                let mut lines = Vec::new();
                lines.push(format!(
                    "\x1b[36m── Pair Remote Node {}\x1b[0m",
                    "─".repeat(width.saturating_sub(22))
                ));
                lines.push(format!("  Ticket: {}", m.ticket.node_id));
                lines.push("  Press Esc to dismiss".to_string());
                lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
                let len = lines.len();
                (lines, len.saturating_sub(1), 0)
            }
            Self::Permission(p) => p.render_lines(width, cursor_mode),
        }
    }
}

#[derive(Clone, Default)]
struct FooterInfo {
    pub model: String,
    pub provider: String,
    pub thinking: Option<String>,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
}

fn make_footer_info(model: &str, provider: &str, thinking: Option<&str>, engine: &AgentEngine) -> FooterInfo {
    let totals = engine.session_usage_totals();
    FooterInfo {
        model: model.to_string(),
        provider: provider.to_string(),
        thinking: thinking.map(str::to_string),
        total_input: totals.total_input,
        total_output: totals.total_output,
        total_cache_read: totals.total_cache_read,
        total_cache_write: totals.total_cache_write,
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
    pub active_modal: &'a mut Option<ActiveModal>,
    pub autocomplete_popup: &'a mut Option<AutocompletePopupView>,
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

async fn print_startup_banner_direct(session: &ReplSession, engine: &AgentEngine) {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let tools = engine.tool_names();
    let mcp = session.config.mcp.servers.keys().cloned().collect::<Vec<_>>();
    let agents = engine.instruction_files().await;
    let skill_names: Vec<String> = skills.iter().map(|s| s.metadata.name.clone()).collect();
    let display = WelcomeDisplay {
        model: session.config.model.clone(),
        provider: session.config.provider.clone(),
        resumed: session.resume_id.is_some(),
        agents,
        tools,
        skills: skill_names,
        mcp,
    };
    let renderer = crate::ui::TerminalRenderer {
        theme: crate::ui::theme::detect_with_config(&session.config.ui),
        ..Default::default()
    };
    renderer.print_welcome(&display);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\n");
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

fn format_footer_path() -> String {
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
    format!("\x1b[90m{path}\x1b[0m")
}

fn format_footer_stats(footer: &FooterInfo, width: usize) -> String {
    let window = ModelRegistry::default().context_window_for(&footer.model, Some(&footer.provider));
    let metrics = FooterMetrics {
        input_tokens: footer.total_input,
        output_tokens: footer.total_output,
        cache_read_tokens: footer.total_cache_read,
        cache_write_tokens: footer.total_cache_write,
        context_tokens: footer.total_input as usize,
        context_window: window,
        total_cost: None,
        tokens_per_second: None,
        quota_summary: None,
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
    if metrics.context_window > 0 {
        parts.push(format!(
            "{:.1}%/{}",
            metrics.context_percent(),
            rho_harness_core::tokens::format_tokens(metrics.context_window as u64)
        ));
    }
    let left = parts.join(" ");

    let plain_len = left.len() + right.len();
    let pad = width.saturating_sub(plain_len);
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

fn build_live_lines(
    state: &RunnerState<'_>,
    footer: &FooterInfo,
    activity: Option<(&Activity, Option<&RunningTool>, usize)>,
    width: usize,
    cursor_mode: CursorMode,
) -> (Vec<String>, usize, usize) {
    if let Some(modal) = state.active_modal.as_ref() {
        return modal.render_lines(width, cursor_mode);
    }

    let mut lines = Vec::new();
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

    if let Some((_, Some(tool), _)) = activity {
        let widget_input = RunningToolWidgetInput {
            tool,
            theme: &state.session.renderer.theme,
            width,
            tools_expanded: state.session.config.ui.tools_expanded.unwrap_or(false),
        };
        let tool_lines = render_running_tool_widget(widget_input);
        lines.extend(tool_lines);
    }

    let ed_lines = state.editor.lines();
    let (c_row, c_col) = state.editor.cursor();
    let ed_start = lines.len();
    for (r, ed_line) in ed_lines.iter().enumerate() {
        let mut row = ed_line.clone();
        if r == c_row && cursor_mode == CursorMode::Software {
            crate::ui::interactive::apply_software_cursor(&mut row, c_col);
        }
        lines.push(row);
    }
    let cursor_row = ed_start + c_row.min(ed_lines.len().saturating_sub(1));

    if let Some(popup) = state.autocomplete_popup.as_ref() {
        for (idx, cand) in popup.candidates.iter().take(5).enumerate() {
            let marker = if idx == popup.selected_index { ">" } else { " " };
            let desc = cand.description.as_deref().unwrap_or("");
            lines.push(format!("\x1b[36m {marker} {:<16} {desc}\x1b[0m", cand.display));
        }
    }

    lines.push(format!("{style}{}{reset}", "─".repeat(width)));
    lines.push(format_footer_path());
    lines.push(format_footer_stats(footer, width));

    (lines, cursor_row, c_col)
}

fn paint_live_region(
    stdout: &mut std::io::Stdout,
    lines: &[String],
    cursor_row: usize,
    cursor_col: usize,
    cursor_mode: CursorMode,
    active_editor: bool,
) -> std::io::Result<()> {
    for (i, line) in lines.iter().enumerate() {
        stdout.write_all(line.as_bytes())?;
        if i + 1 < lines.len() {
            stdout.write_all(b"\r\n")?;
        }
    }
    let rows_up = lines.len().saturating_sub(1).saturating_sub(cursor_row);
    if rows_up > 0 {
        write!(stdout, "\x1b[{rows_up}A")?;
    }
    if cursor_col > 0 {
        write!(stdout, "\r\x1b[{cursor_col}C")?;
    } else {
        stdout.write_all(b"\r")?;
    }
    if cursor_mode == CursorMode::Hardware && active_editor {
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
    extra_output: Option<&str>,
    tracker: &mut OutputTracker,
) -> std::io::Result<()> {
    let width = crate::ui::terminal_width() as usize;
    let cursor_mode = state.session.renderer.theme.cursor_mode;
    let (lines, c_row, c_col) = build_live_lines(state, footer, activity, width, cursor_mode);

    let mut stdout = std::io::stdout();
    stdout.write_all(CSI_SYNC_BEGIN)?;
    erase_live_region(&mut stdout, state.prev_lines_count, state.prev_cursor_row)?;

    if let Some(out) = extra_output
        && !out.is_empty()
    {
        tracker.restore_cursor(&mut stdout, width)?;
        let normalized = terminal_newlines(out);
        stdout.write_all(normalized.as_bytes())?;
        tracker.update(&normalized);
        if tracker.is_open() {
            stdout.write_all(b"\r\n")?;
        }
    }

    let active_editor = state.active_modal.is_none();
    paint_live_region(&mut stdout, &lines, c_row, c_col, cursor_mode, active_editor)?;
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
            if matches!(item, TranscriptItem::AssistantText(_) | TranscriptItem::Thinking(_)) {
                return;
            }
            if matches!(item, TranscriptItem::Tool(_)) {
                *running_tool = None;
            }
            let width = crate::ui::terminal_width() as usize;
            let input = TranscriptRenderInput {
                item: &item,
                theme: &session.renderer.theme,
                width,
                tools_expanded: session.config.ui.tools_expanded.unwrap_or(false),
                hide_thinking: session.config.ui.hide_thinking.unwrap_or(false),
            };
            output.push_str(&crate::ui::interactive::render_transcript_item(input));
            output.push('\n');
        }
        UiEvent::Activity(act) => {
            *activity = act;
        }
        _ => {}
    }
}

fn apply_modal_selection(modal: &ActiveModal, session: &mut ReplSession) {
    if let ActiveModal::Standard(std_modal) = modal
        && let Some(opt) = std_modal.state.selected_option()
    {
        let val = opt.value.clone();
        match std_modal.state.title.as_str() {
            "Select Model" => {
                let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
                if let Some(m) = discovered.iter().find(|d| d.id == val) {
                    session.config.model = m.id.clone();
                    session.config.provider = m.provider.clone();
                } else {
                    session.config.model = val;
                }
            }
            "Select Thinking Level" => {
                session.config.thinking_level = Some(val);
            }
            _ => {}
        }
    }
}

fn handle_modal_input(mut modal: ActiveModal, key: KeyEvent, session: &mut ReplSession) -> Option<ActiveModal> {
    if key.code == KeyCode::Enter {
        apply_modal_selection(&modal, session);
        None
    } else {
        modal.handle_key(key);
        if modal.is_open() { Some(modal) } else { None }
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

async fn handle_input_action(
    action: InputAction,
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
) -> Option<bool> {
    match action {
        InputAction::Clear => {
            state.editor.clear();
            Some(false)
        }
        InputAction::Cancel => {
            if !state.editor.is_empty() {
                state.editor.clear();
            }
            Some(false)
        }
        InputAction::EndOfInput => Some(state.editor.is_empty()),
        InputAction::ModelSelect => {
            *state.active_modal = Some(build_model_modal(state.session));
            Some(false)
        }
        InputAction::ModelCycleForward => {
            cycle_model(state, engine, 1).await;
            Some(false)
        }
        InputAction::ModelCycleBackward => {
            cycle_model(state, engine, -1).await;
            Some(false)
        }
        InputAction::ThinkingCycle => {
            cycle_thinking_level(state, engine).await;
            Some(false)
        }
        InputAction::ThinkingToggle => {
            let hide = !state.session.config.ui.hide_thinking.unwrap_or(false);
            state.session.config.ui.hide_thinking = Some(hide);
            Some(false)
        }
        InputAction::ToggleExpandTools => {
            let exp = !state.session.config.ui.tools_expanded.unwrap_or(false);
            state.session.config.ui.tools_expanded = Some(exp);
            Some(false)
        }
        #[cfg(unix)]
        InputAction::Suspend => {
            unsafe {
                libc::raise(libc::SIGTSTP);
            }
            Some(false)
        }
        _ => None,
    }
}

async fn handle_key_cycle(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    key: KeyEvent,
    completions: &CompletionEngine,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
) -> Result<bool> {
    if let Some(modal) = state.active_modal.take() {
        if key.code == KeyCode::Esc {
            return Ok(false);
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            return Ok(false);
        }
        *state.active_modal = handle_modal_input(modal, key, state.session);
        state.session.sync_engine_model(engine).await;
        return Ok(false);
    }

    if let Some(popup) = state.autocomplete_popup.take() {
        let (next_popup, consumed) = handle_popup_input(popup, key, state.editor);
        *state.autocomplete_popup = next_popup;
        if consumed {
            return Ok(false);
        }
    }

    let action = map_key(key);
    if let Some(should_exit) = handle_input_action(action, state, engine).await {
        return Ok(should_exit);
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
        let prompt = state.editor.text().to_string();
        if prompt.trim().is_empty() {
            return Ok(false);
        }
        state.editor.clear();
        return handle_submission(state, engine, prompt, ui_events, events).await;
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
}

async fn init_live_context(session: &mut ReplSession) -> Result<LiveContext> {
    let engine = init_live_engine(session).await?;
    print_startup_banner_direct(session, &engine).await;

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
    let completions = build_completions();

    Ok(LiveContext {
        engine,
        ui_events,
        editor,
        history,
        completions,
    })
}

pub async fn run_unified_live(session: &mut ReplSession) -> Result<()> {
    let mut ctx = init_live_context(session).await?;
    let _guard = TerminalGuard::enter()?;

    let mut active_modal: Option<ActiveModal> = None;
    let mut autocomplete_popup: Option<AutocompletePopupView> = None;
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    let mut tracker = OutputTracker::default();

    let mut state = RunnerState {
        session,
        editor: &mut ctx.editor,
        history: &mut ctx.history,
        active_modal: &mut active_modal,
        autocomplete_popup: &mut autocomplete_popup,
        prev_lines_count: 0,
        prev_cursor_row: 0,
    };

    let footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        &ctx.engine,
    );
    refresh_display(&mut state, &footer, None, None, &mut tracker)?;

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else {
                    break;
                };

                match event {
                    Event::Resize(w, _) => {
                        let width = (w as usize).max(1);
                        state.session.renderer.set_width(width);
                        let mut stdout = std::io::stdout();
                        let _ = stdout.write_all(b"\r\x1b[J");
                        let _ = stdout.flush();
                        state.prev_lines_count = 0;
                        state.prev_cursor_row = 0;
                        let footer = make_footer_info(
                            &state.session.config.model,
                            &state.session.config.provider,
                            state.session.config.thinking_level.as_deref(),
                            &ctx.engine,
                        );
                        refresh_display(&mut state, &footer, None, None, &mut tracker)?;
                    }
                    Event::Key(key) => {
                        let should_exit = handle_key_cycle(
                            &mut state,
                            &mut ctx.engine,
                            key,
                            &ctx.completions,
                            &mut ctx.ui_events,
                            &mut events,
                        ).await?;
                        if should_exit {
                            break;
                        }
                        let footer = make_footer_info(
                            &state.session.config.model,
                            &state.session.config.provider,
                            state.session.config.thinking_level.as_deref(),
                            &ctx.engine,
                        );
                        refresh_display(&mut state, &footer, None, None, &mut tracker)?;
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

fn build_settings_modal(session: &ReplSession) -> ActiveModal {
    let settings = SettingsState {
        hide_thinking: session.config.ui.hide_thinking.unwrap_or(false),
        tools_expanded: session.config.ui.tools_expanded.unwrap_or(true),
        vim_mode: session.config.editor.is_vim(),
        show_version_banner: true,
    };
    ActiveModal::Standard(StandardModalView::settings(&settings))
}

fn build_model_modal(session: &ReplSession) -> ActiveModal {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let models = discovered
        .into_iter()
        .map(|m| ModelCapability {
            id: m.id.clone(),
            provider: m.provider.clone(),
            display_name: format!("{} ({})", m.id, m.provider),
            context_tokens: 128_000,
            supports_reasoning: false,
            is_local: m.provider == "ollama",
        })
        .collect();
    let registry = ModelRegistry::new(models, session.config.model.clone());
    ActiveModal::Standard(StandardModalView::model(&registry))
}

async fn handle_interactive_command(
    cmd: &str,
    session: &mut ReplSession,
    engine: &AgentEngine,
    active_modal: &mut Option<ActiveModal>,
) -> bool {
    match cmd {
        "/model" => {
            *active_modal = Some(build_model_modal(session));
            true
        }
        "/thinking" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::thinking(
                session.config.thinking_level.as_deref(),
            )));
            true
        }
        "/login" => {
            *active_modal = Some(ActiveModal::Standard(StandardModalView::auth(PROVIDER_DEFS, None)));
            true
        }
        "/mcp" => {
            let servers: Vec<McpServerInfo> = session
                .config
                .mcp
                .servers
                .iter()
                .map(|(name, cfg)| McpServerInfo {
                    name: name.clone(),
                    command: cfg.command.clone().unwrap_or_default(),
                    enabled: cfg.enabled,
                    status_message: String::new(),
                    tools_count: 0,
                })
                .collect();
            let mcp_state = McpModalState::new(servers);
            *active_modal = Some(ActiveModal::Standard(StandardModalView::mcp(&mcp_state)));
            true
        }
        "/skill" => {
            let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
            let items: Vec<SkillInfo> = skills
                .into_iter()
                .map(|s| SkillInfo {
                    name: s.metadata.name,
                    description: s.metadata.description,
                    origin: format!("{:?}", s.origin),
                    path: None,
                })
                .collect();
            let skill_state = SkillModalState::new(items);
            *active_modal = Some(ActiveModal::Standard(StandardModalView::skill(&skill_state)));
            true
        }
        "/session" => {
            let summaries = list_session_summaries_async(&session.config.sessions_dir)
                .await
                .unwrap_or_default();
            *active_modal = Some(ActiveModal::Standard(StandardModalView::session(
                &summaries,
                Some(&engine.session_manager.session_id),
            )));
            true
        }
        "/pair" => {
            *active_modal = Some(ActiveModal::RemotePair(RemotePairModalView::new(RhoTicket {
                node_id: "rho-local-node".to_string(),
                relay_url: None,
            })));
            true
        }
        "/settings" => {
            *active_modal = Some(build_settings_modal(session));
            true
        }
        _ => false,
    }
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
    if rest.is_empty() && handle_interactive_command(cmd, state.session, engine, state.active_modal).await {
        return Ok(false);
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

fn print_initial_prompt(
    state: &mut RunnerState<'_>,
    prompt: &str,
    footer: &FooterInfo,
    tracker: &mut OutputTracker,
) -> Result<()> {
    let width = crate::ui::terminal_width() as usize;
    let user_box = state.session.renderer.theme.user_block(width).render_plain(prompt);
    let pending_scrollback = format!("{user_box}\n\n");
    refresh_display(state, footer, None, Some(&pending_scrollback), tracker)?;
    Ok(())
}

async fn finish_turn_execution(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    tracker: &mut OutputTracker,
) -> Result<()> {
    state.session.sync_engine_model(engine).await;
    engine.refresh_quota().await;
    broadcast_turn_completion(engine);

    tracker.clear();
    let updated_footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    refresh_display(state, &updated_footer, None, None, tracker)?;
    Ok(())
}

fn handle_turn_key_input(
    key: KeyEvent,
    state: &mut RunnerState<'_>,
    steering: &SharedSteeringQueue,
    cancellation: &CancellationSignal,
    active_responder: &mut Option<InteractionResponder>,
    pending_scrollback: &mut String,
) -> bool {
    if let Some(ActiveModal::Permission(mut p)) = state.active_modal.take() {
        p.handle_key(key);
        if let Some(resp) = p.resolved.take() {
            if let Some(responder) = active_responder.take() {
                let _ = responder.respond(resp);
            }
        } else {
            *state.active_modal = Some(ActiveModal::Permission(p));
        }
        return false;
    }
    let action = map_key(key);
    match action {
        InputAction::Cancel => {
            cancellation.cancel();
            rho_engine::process::kill_all_tracked_processes();
            pending_scrollback.push_str("\nCanceled.\n");
            true
        }
        InputAction::Clear => {
            state.editor.clear();
            false
        }
        _ => {
            if !state.editor.handle_key(key) {
                let steering_text = state.editor.text().to_string();
                if !steering_text.trim().is_empty() {
                    steering.enqueue(steering_text.clone());
                    state.editor.clear();
                    pending_scrollback.push_str(&format!("\x1b[36m[Steering queued: {steering_text}]\x1b[0m\n"));
                }
            }
            false
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
    pending_scrollback: &mut String,
    tracker: &mut OutputTracker,
) -> Result<()> {
    if !pending_scrollback.is_empty() {
        let out = std::mem::take(pending_scrollback);
        refresh_display(state, footer, Some(activity_meta), Some(&out), tracker)?;
    } else {
        refresh_display(state, footer, Some(activity_meta), None, tracker)?;
    }
    Ok(())
}

#[derive(Default)]
struct TurnStreamState {
    pub activity: Activity,
    pub running_tool: Option<RunningTool>,
    pub scrollback: String,
    pub responder: Option<InteractionResponder>,
    pub tick_counter: usize,
    pub spinner_frame: usize,
}

fn handle_turn_event(ui_ev: UiEvent, state: &mut RunnerState<'_>, stream: &mut TurnStreamState) {
    match ui_ev {
        UiEvent::Interaction { prompt: p, responder } => {
            let modal = PermissionModal::new(p);
            *state.active_modal = Some(ActiveModal::Permission(Box::new(modal)));
            stream.responder = Some(responder);
        }
        other => {
            drain_ui_event(
                other,
                &mut stream.scrollback,
                &mut stream.activity,
                &mut stream.running_tool,
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
    tracker: &mut OutputTracker,
) -> Result<()> {
    state.session.renderer.set_width(width);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\r\x1b[J");
    let _ = stdout.flush();
    state.prev_lines_count = 0;
    state.prev_cursor_row = 0;
    let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
    refresh_display(state, footer, Some(activity_meta), None, tracker)?;
    Ok(())
}

fn finalize_turn_result<T>(
    res: Result<T>,
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &mut TurnStreamState,
    tracker: &mut OutputTracker,
) -> Result<()> {
    if let Err(ref err) = res {
        stream.scrollback.push_str(&format!("\nError: {err}\n"));
    }
    if !stream.scrollback.ends_with('\n') {
        stream.scrollback.push('\n');
    }
    stream.scrollback.push('\n');
    refresh_display(state, footer, None, Some(&stream.scrollback), tracker)?;
    stream.scrollback.clear();
    Ok(())
}

async fn execute_agent_turn(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    prompt: &str,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
) -> Result<()> {
    while ui_events.try_recv().is_ok() {}

    let mut tracker = OutputTracker::default();
    let footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    print_initial_prompt(state, prompt, &footer, &mut tracker)?;

    let cancellation = Arc::new(CancellationSignal::default());
    let (steering, broadcast) = init_turn_channels(engine, state.session.renderer.clone());
    let request = TurnRequest::new(prompt)
        .with_cancellation(&cancellation)
        .with_steering(steering.clone());
    broadcast_turn_start(prompt);

    let mut turn_future = Box::pin(engine.run_turn(request, broadcast));
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    let mut stream = TurnStreamState::default();

    loop {
        tokio::select! {
            res = &mut turn_future => {
                while let Ok(ui_ev) = ui_events.try_recv() {
                    drain_ui_event(ui_ev, &mut stream.scrollback, &mut stream.activity, &mut stream.running_tool, state.session);
                }
                finalize_turn_result(res, state, &footer, &mut stream, &mut tracker)?;
                break;
            }
            Some(ui_ev) = ui_events.recv() => {
                handle_turn_event(ui_ev, state, &mut stream);
            }
            _ = ticker.tick() => {
                stream.tick_counter += 1;
                if stream.tick_counter.is_multiple_of(5) {
                    stream.spinner_frame = (stream.spinner_frame + 1) % 10;
                }
                let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
                flush_or_refresh_turn_tick(state, &footer, activity_meta, &mut stream.scrollback, &mut tracker)?;
            }
            maybe_key = events.next() => {
                match maybe_key {
                    Some(Ok(Event::Resize(w, _))) => {
                        handle_turn_resize((w as usize).max(1), state, &footer, &stream, &mut tracker)?;
                    }
                    Some(Ok(Event::Key(key))) => {
                        let cancelled = handle_turn_key_input(
                            key,
                            state,
                            &steering,
                            &cancellation,
                            &mut stream.responder,
                            &mut stream.scrollback,
                        );
                        if cancelled {
                            let _ = engine.record_cancellation("operator interrupt").await;
                            refresh_display(state, &footer, None, Some(&stream.scrollback), &mut tracker)?;
                            stream.scrollback.clear();
                            break;
                        }
                        let activity_meta = (&stream.activity, stream.running_tool.as_ref(), stream.spinner_frame);
                        refresh_display(state, &footer, Some(activity_meta), None, &mut tracker)?;
                    }
                    _ => {}
                }
            }
        }
    }
    drop(turn_future);
    crate::platform::remote::set_active_steering(None);
    *state.active_modal = None;

    finish_turn_execution(state, engine, &mut tracker).await?;
    Ok(())
}

async fn handle_submission(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    prompt: String,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
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

    execute_agent_turn(state, engine, trimmed, ui_events, events).await?;
    Ok(false)
}
