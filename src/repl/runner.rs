use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use rho_harness_core::presentation::WelcomeDisplay;
use rho_harness_core::rpc::protocol::RpcEvent;
use rho_harness_core::session::list_session_summaries_async;
use rho_ui_core::autocomplete::CompletionEngine;
use rho_ui_core::keymap::{InputAction, map_key};
use rho_ui_core::modal::{
    McpModalState, McpServerInfo, ModelCapability, ModelRegistry, SettingsState, SkillInfo, SkillModalState,
};
use rho_ui_core::permission::{PERMISSION_ACTIONS, PermissionAction, PermissionPromptState};
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
    Activity, InteractionResponder, InteractionResponse, InteractiveUi, OutputEvent, TranscriptItem,
    TranscriptRenderInput, UiEvent,
};
use crate::ui::modal::{AutocompletePopupView, PermissionPromptView, RemotePairModalView, StandardModalView};
use crate::ui::terminal::TerminalGuard;
use crate::ui::widgets::StreamingSpinner;
use crate::ui::{ModalView, PromptEditor};

const CSI_SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const CSI_SYNC_END: &[u8] = b"\x1b[?2026l";
const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

enum ActiveModal {
    Standard(StandardModalView),
    RemotePair(RemotePairModalView),
    Permission(Box<PermissionPromptView>),
}

impl ActiveModal {
    fn is_open(&self) -> bool {
        match self {
            Self::Standard(m) => m.state.is_open,
            Self::RemotePair(m) => m.is_open,
            Self::Permission(p) => p.resolved_action.is_none(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self {
            Self::Standard(m) => m.handle_key(key),
            Self::RemotePair(m) => m.handle_key(key),
            Self::Permission(p) => p.handle_key(key),
        }
    }

    fn render_lines(&self, width: usize) -> Vec<String> {
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
                if m.state.filtered_options.is_empty() {
                    lines.push("\x1b[90m  No matching options\x1b[0m".to_string());
                } else {
                    for (idx, opt) in m.state.filtered_options.iter().take(8).enumerate() {
                        let marker = if idx == m.state.selected_index { ">" } else { " " };
                        let active = if opt.is_active { " ✓" } else { "" };
                        let desc = opt.description.as_deref().unwrap_or("");
                        let line = format!(" {marker} {:<18} {desc}{active}", opt.label);
                        if idx == m.state.selected_index {
                            lines.push(format!("\x1b[1;36m{line}\x1b[0m"));
                        } else {
                            lines.push(format!("\x1b[90m{line}\x1b[0m"));
                        }
                    }
                }
                lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
                lines
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
                lines
            }
            Self::Permission(p) => {
                let mut lines = Vec::new();
                let sep = "─".repeat(width.saturating_sub(26));
                lines.push(format!("\x1b[33m── Permission Required {sep}\x1b[0m"));
                for line in p.prompt.command_display.lines().take(4) {
                    lines.push(format!("  {line}"));
                }
                if p.prompt.is_editing {
                    lines.push(format!(" \x1b[1;33m[EDITING]\x1b[0m {}", p.editor.text()));
                } else {
                    let mut actions = Vec::new();
                    for (i, (lbl, _)) in PERMISSION_ACTIONS.iter().enumerate() {
                        let num = i + 1;
                        if i == p.prompt.selected_index {
                            actions.push(format!("\x1b[1;33m[{num}. {lbl}]\x1b[0m"));
                        } else {
                            actions.push(format!("\x1b[90m{num}. {lbl}\x1b[0m"));
                        }
                    }
                    lines.push(format!(" {}", actions.join("  ")));
                }
                lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
                lines
            }
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

fn render_editor_row(text: &str, _cursor_col: usize) -> String {
    text.to_string()
}

fn build_live_lines(
    state: &RunnerState<'_>,
    footer: &FooterInfo,
    activity: Option<(&Activity, Option<&str>, usize)>,
    width: usize,
) -> (Vec<String>, usize, usize) {
    if let Some(modal) = state.active_modal.as_ref() {
        let lines = modal.render_lines(width);
        let len = lines.len();
        return (lines, len.saturating_sub(1), 0);
    }

    let mut lines = Vec::new();
    let top_divider = match activity {
        Some((act, running_tool, frame)) => {
            let spinner = StreamingSpinner::current_frame(frame);
            let label = if let Some(tool) = running_tool {
                format!("Running: {tool}")
            } else if matches!(act, Activity::Thinking) {
                "Thinking...".to_string()
            } else {
                "Working...".to_string()
            };
            let prefix = format!("── {spinner} {label} ");
            let rem = width.saturating_sub(prefix.chars().count());
            format!("\x1b[38;2;60;60;60m{prefix}{}\x1b[0m", "─".repeat(rem))
        }
        None => format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)),
    };
    lines.push(top_divider);

    let text = state.editor.text();
    let (_c_row, c_col) = state.editor.cursor();
    lines.push(render_editor_row(&text, c_col));

    if let Some(popup) = state.autocomplete_popup.as_ref() {
        for (idx, cand) in popup.candidates.iter().take(5).enumerate() {
            let marker = if idx == popup.selected_index { ">" } else { " " };
            let desc = cand.description.as_deref().unwrap_or("");
            lines.push(format!("\x1b[36m {marker} {:<16} {desc}\x1b[0m", cand.display));
        }
    }

    lines.push(format!("\x1b[38;2;60;60;60m{}\x1b[0m", "─".repeat(width)));
    lines.push(format_footer_path());
    lines.push(format_footer_stats(footer, width));

    let cursor_row = 1;
    (lines, cursor_row, c_col)
}

fn paint_live_region(
    stdout: &mut std::io::Stdout,
    lines: &[String],
    cursor_row: usize,
    cursor_col: usize,
    show_cursor: bool,
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
    if show_cursor {
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
    Ok(())
}

fn refresh_display(
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    activity: Option<(&Activity, Option<&str>, usize)>,
    extra_output: Option<&str>,
    tracker: &mut OutputTracker,
) -> std::io::Result<()> {
    let width = crate::ui::terminal_width() as usize;
    let (lines, c_row, c_col) = build_live_lines(state, footer, activity, width);

    let show_cursor = state.active_modal.is_none();
    let mut stdout = std::io::stdout();
    stdout.write_all(CSI_SYNC_BEGIN)?;
    stdout.write_all(b"\x1b[?25l")?;
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

    paint_live_region(&mut stdout, &lines, c_row, c_col, show_cursor)?;
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
    tool: &mut Option<String>,
    session: &ReplSession,
) {
    match ev {
        UiEvent::Output(OutputEvent::Text(t) | OutputEvent::StreamText(t)) => {
            output.push_str(&t);
        }
        UiEvent::Transcript(item) => {
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
            output.push_str(&crate::ui::interactive::render_transcript_item(input));
        }
        UiEvent::Activity(act) => {
            *activity = act;
        }
        UiEvent::RunningTool(t) => {
            *tool = t;
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
    state
        .session
        .renderer
        .print_notice(&format!("Switched model to {} ({})\n", next.id, next.provider));
}

async fn cycle_thinking_level(state: &mut RunnerState<'_>, engine: &mut AgentEngine) {
    let current = state.session.config.thinking_level.as_deref().unwrap_or("off");
    let current_idx = THINKING_LEVELS.iter().position(|&l| l == current).unwrap_or(0);
    let next_idx = (current_idx + 1) % THINKING_LEVELS.len();
    let next = THINKING_LEVELS[next_idx];
    state.session.config.thinking_level = Some(next.to_string());
    state.session.sync_engine_model(engine).await;
    state
        .session
        .renderer
        .print_notice(&format!("Set thinking level to {next}\n"));
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
            state
                .session
                .renderer
                .print_notice(&format!("Thinking: {}\n", if hide { "hidden" } else { "visible" }));
            Some(false)
        }
        InputAction::ToggleExpandTools => {
            let exp = !state.session.config.ui.tools_expanded.unwrap_or(false);
            state.session.config.ui.tools_expanded = Some(exp);
            state
                .session
                .renderer
                .print_notice(&format!("Tools: {}\n", if exp { "expanded" } else { "collapsed" }));
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

pub async fn run_unified_live(session: &mut ReplSession) -> Result<()> {
    let mut engine = init_live_engine(session).await?;
    print_startup_banner_direct(session, &engine).await;

    let (ui, mut ui_events) = InteractiveUi::channel();
    session.renderer = crate::ui::TerminalRenderer::with_ui(ui);
    session.renderer.theme = crate::ui::theme::detect_with_config(&session.config.ui);
    session.renderer.set_width(crate::ui::terminal_width() as usize);

    let _guard = TerminalGuard::enter()?;

    let mode = if session.config.editor.is_vim() {
        EditorMode::Vim
    } else {
        EditorMode::Default
    };
    let mut editor = TextAreaEditor::new(mode);
    let mut history = load_history(session).await;
    let completions = build_completions();

    let mut active_modal: Option<ActiveModal> = None;
    let mut autocomplete_popup: Option<AutocompletePopupView> = None;
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    let mut tracker = OutputTracker::default();

    let mut state = RunnerState {
        session,
        editor: &mut editor,
        history: &mut history,
        active_modal: &mut active_modal,
        autocomplete_popup: &mut autocomplete_popup,
        prev_lines_count: 0,
        prev_cursor_row: 0,
    };

    let footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        &engine,
    );
    refresh_display(&mut state, &footer, None, None, &mut tracker)?;

    loop {
        let footer = make_footer_info(
            &state.session.config.model,
            &state.session.config.provider,
            state.session.config.thinking_level.as_deref(),
            &engine,
        );

        tokio::select! {
            _ = ticker.tick() => {}
            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else {
                    break;
                };

                match event {
                    Event::Resize(_, _) => {
                        let width = crate::ui::terminal_width() as usize;
                        state.session.renderer.set_width(width);
                        refresh_display(&mut state, &footer, None, None, &mut tracker)?;
                    }
                    Event::Key(key) => {
                        let should_exit = handle_key_cycle(
                            &mut state,
                            &mut engine,
                            key,
                            &completions,
                            &mut ui_events,
                            &mut events,
                        ).await?;
                        if should_exit {
                            break;
                        }
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
        state
            .session
            .renderer
            .print_notice(&format!("Switched model to {rest}\n"));
        return Ok(false);
    }
    if cmd == "/thinking" && !rest.is_empty() {
        state.session.config.thinking_level = Some(rest.to_string());
        state.session.sync_engine_model(engine).await;
        state
            .session
            .renderer
            .print_notice(&format!("Set thinking level to {rest}\n"));
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

fn handle_permission_key(
    mut p: PermissionPromptView,
    key: KeyEvent,
    active_responder: &mut Option<InteractionResponder>,
) -> Option<ActiveModal> {
    if key.code == KeyCode::Esc {
        if let Some(resp) = active_responder.take() {
            let _ = resp.respond(InteractionResponse::Cancelled);
        }
        return None;
    }
    p.handle_key(key);
    if let Some(action) = p.resolved_action.take() {
        let response = match action {
            PermissionAction::AllowOnce => InteractionResponse::Selected(0),
            PermissionAction::AllowAlways => InteractionResponse::Selected(1),
            PermissionAction::Deny { reason } => match reason {
                Some(r) => InteractionResponse::SelectedWithInput { index: 2, text: r },
                None => InteractionResponse::Selected(2),
            },
            PermissionAction::Edit { mutated_command } => InteractionResponse::SelectedWithInput {
                index: 3,
                text: mutated_command,
            },
        };
        if let Some(resp) = active_responder.take() {
            let _ = resp.respond(response);
        }
        None
    } else {
        Some(ActiveModal::Permission(Box::new(p)))
    }
}

fn handle_turn_key_input(
    key: KeyEvent,
    state: &mut RunnerState<'_>,
    steering: &SharedSteeringQueue,
    cancellation: &CancellationSignal,
    active_responder: &mut Option<InteractionResponder>,
    pending_scrollback: &mut String,
) -> bool {
    if let Some(ActiveModal::Permission(p)) = state.active_modal.take() {
        *state.active_modal = handle_permission_key(*p, key, active_responder);
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

fn setup_turn_execution<'a>(
    engine: &'a AgentEngine,
    renderer: crate::ui::TerminalRenderer,
    prompt: &'a str,
    cancellation: &'a Arc<CancellationSignal>,
) -> (
    Arc<SharedSteeringQueue>,
    TurnRequest<'a>,
    Arc<dyn rho_harness_core::presentation::Presenter>,
) {
    let steering = Arc::new(SharedSteeringQueue::new(engine.config.steering_mode));
    crate::platform::remote::set_active_steering(Some(steering.clone()));
    let request = TurnRequest::new(prompt)
        .with_cancellation(cancellation)
        .with_steering(steering.clone());
    let broadcast: Arc<dyn rho_harness_core::presentation::Presenter> = Arc::new(
        crate::ui::render::BroadcastPresenter::new(Arc::new(renderer), crate::platform::remote::PEER_REGISTRY.clone()),
    );
    (steering, request, broadcast)
}

fn flush_or_refresh_turn_tick(
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    activity_meta: (&Activity, Option<&str>, usize),
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

fn handle_turn_event(
    ui_ev: UiEvent,
    state: &mut RunnerState<'_>,
    active_responder: &mut Option<InteractionResponder>,
    pending_scrollback: &mut String,
    current_activity: &mut Activity,
    current_tool: &mut Option<String>,
) {
    match ui_ev {
        UiEvent::Interaction { prompt: p, responder } => {
            let perm_state = PermissionPromptState {
                is_active: true,
                tool_name: "tool".to_string(),
                command_display: p.body.clone(),
                arguments: serde_json::Value::Null,
                selected_index: p.initial_selection,
                custom_deny_reason: String::new(),
                edited_command: p.body.clone(),
                is_editing: false,
            };
            let view = PermissionPromptView::new(perm_state);
            *state.active_modal = Some(ActiveModal::Permission(Box::new(view)));
            *active_responder = Some(responder);
        }
        other => {
            drain_ui_event(other, pending_scrollback, current_activity, current_tool, state.session);
        }
    }
}

async fn execute_agent_turn(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    prompt: &str,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
) -> Result<()> {
    let mut tracker = OutputTracker::default();
    let footer = make_footer_info(
        &state.session.config.model,
        &state.session.config.provider,
        state.session.config.thinking_level.as_deref(),
        engine,
    );
    print_initial_prompt(state, prompt, &footer, &mut tracker)?;

    let renderer = state.session.renderer.clone();
    let cancellation = Arc::new(CancellationSignal::default());
    let (steering, request, broadcast) = setup_turn_execution(engine, renderer, prompt, &cancellation);
    broadcast_turn_start(prompt);

    let mut turn_future = Box::pin(engine.run_turn(request, broadcast));
    let mut ticker = tokio::time::interval(Duration::from_millis(16));
    let mut spinner_frame = 0;
    let mut current_activity = Activity::Working;
    let mut current_tool: Option<String> = None;
    let mut pending_scrollback = String::new();
    let mut active_responder: Option<InteractionResponder> = None;

    loop {
        tokio::select! {
            res = &mut turn_future => {
                while let Ok(ui_ev) = ui_events.try_recv() {
                    drain_ui_event(ui_ev, &mut pending_scrollback, &mut current_activity, &mut current_tool, state.session);
                }
                if let Err(ref err) = res {
                    pending_scrollback.push_str(&format!("\nError: {err}\n"));
                }
                pending_scrollback.push('\n');
                refresh_display(state, &footer, None, Some(&pending_scrollback), &mut tracker)?;
                pending_scrollback.clear();
                break;
            }
            Some(ui_ev) = ui_events.recv() => {
                handle_turn_event(
                    ui_ev,
                    state,
                    &mut active_responder,
                    &mut pending_scrollback,
                    &mut current_activity,
                    &mut current_tool,
                );
            }
            _ = ticker.tick() => {
                spinner_frame = (spinner_frame + 1) % 10;
                let activity_meta = (&current_activity, current_tool.as_deref(), spinner_frame);
                flush_or_refresh_turn_tick(state, &footer, activity_meta, &mut pending_scrollback, &mut tracker)?;
            }
            maybe_key = events.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_key {
                    let cancelled = handle_turn_key_input(
                        key,
                        state,
                        &steering,
                        &cancellation,
                        &mut active_responder,
                        &mut pending_scrollback,
                    );
                    if cancelled {
                        let _ = engine.record_cancellation("operator interrupt").await;
                        refresh_display(state, &footer, None, Some(&pending_scrollback), &mut tracker)?;
                        pending_scrollback.clear();
                        break;
                    }
                    let activity_meta = (&current_activity, current_tool.as_deref(), spinner_frame);
                    refresh_display(state, &footer, Some(activity_meta), None, &mut tracker)?;
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
