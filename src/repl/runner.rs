use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use rho_harness_core::presentation::InteractionPrompt;
use rho_harness_core::rpc::protocol::RpcEvent;
use rho_harness_core::session::list_session_summaries_async;
use rho_ui_core::autocomplete::CompletionEngine;
use rho_ui_core::keymap::{InputAction, map_key};
use rho_ui_core::modal::ModelRegistry;
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
    Activity, InInputModalInput, InteractionResponder, InteractionResponse, InteractiveUi, ModalMode, ModalOption,
    ModalState, OptionLayout, OutputEvent, RunningTool, RunningToolWidgetInput, TranscriptItem, TranscriptRenderInput,
    UiEvent, modal_banner_title, modal_hint, modal_top_divider, render_in_input_modal, render_running_tool_widget,
};
use crate::ui::modal::AutocompletePopupView;
use crate::ui::terminal::TerminalGuard;
use crate::ui::theme::CursorMode;
use crate::ui::widgets::StreamingSpinner;

const CSI_SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const CSI_SYNC_END: &[u8] = b"\x1b[?2026l";
const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

fn build_interaction_state(prompt: InteractionPrompt) -> ModalState {
    let options = prompt
        .options
        .into_iter()
        .map(|o| ModalOption {
            label: o.label,
            description: o.description,
            input: o.input,
        })
        .collect::<Vec<_>>();
    let is_empty = options.is_empty();
    let mut state = ModalState::new(prompt.title, prompt.body, options)
        .with_custom(prompt.allow_custom)
        .with_option_layout(prompt.option_layout);
    state.selected = prompt.initial_selection.min(state.options.len().saturating_sub(1));
    if is_empty || (prompt.allow_custom && state.options.is_empty()) || prompt.initial_text.is_some() {
        state.enter_input_mode("input");
    }
    if let Some(prefill) = prompt.initial_text {
        state.input.set_text(prefill);
    }
    state
}

fn apply_modal_selection(modal: &ModalState, session: &mut ReplSession) {
    let Some(opt) = modal.selected_option() else {
        return;
    };
    match modal.title.as_str() {
        "Select Model" => {
            let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
            if let Some(m) = discovered.iter().find(|d| d.id == opt.label) {
                session.config.model = m.id.clone();
                session.config.provider = m.provider.clone();
            } else {
                session.config.model = opt.label.clone();
            }
        }
        "Select Thinking Level" => {
            let level = opt.label.trim().to_string();
            session.config.thinking_level = if level == "off" { None } else { Some(level) };
        }
        _ => {}
    }
}

fn apply_modal_input_key(input: &mut crate::ui::interactive::EditorState, key: KeyEvent) {
    match key.code {
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left => input.move_left(),
        KeyCode::Right => input.move_right(),
        KeyCode::Home => input.move_to_start(),
        KeyCode::End => input.move_to_end(),
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            input.insert(c);
        }
        _ => {}
    }
}

fn handle_modal_input_mode(
    modal: &mut ModalState,
    key: KeyEvent,
    active_responder: &mut Option<InteractionResponder>,
) -> bool {
    match key.code {
        KeyCode::Enter => {
            let text = modal.input.text().to_string();
            if let Some(resp) = active_responder.take() {
                let index = modal.input_option.unwrap_or(modal.selected);
                let response = if !text.is_empty() {
                    InteractionResponse::SelectedWithInput { index, text }
                } else {
                    InteractionResponse::Selected(index)
                };
                let _ = resp.respond(response);
            }
            true
        }
        _ => {
            apply_modal_input_key(&mut modal.input, key);
            false
        }
    }
}

fn handle_modal_nav_key(modal: &mut ModalState, key: KeyEvent) {
    if modal.option_layout == OptionLayout::Horizontal {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => modal.select_previous(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => modal.select_next(),
            KeyCode::Up | KeyCode::Char('k') => modal.scroll_body_up(),
            KeyCode::Down | KeyCode::Char('j') => modal.scroll_body_down(usize::MAX),
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Up | KeyCode::BackTab => modal.select_previous(),
            KeyCode::Down | KeyCode::Tab => modal.select_next(),
            KeyCode::Char('k') if !modal.is_searchable || modal.filter_query.is_empty() => {
                modal.select_previous();
            }
            KeyCode::Char('j') if !modal.is_searchable || modal.filter_query.is_empty() => {
                modal.select_next();
            }
            _ => {}
        }
    }
}

fn handle_modal_action_key(
    modal: &mut ModalState,
    key: KeyEvent,
    session: &mut ReplSession,
    active_responder: &mut Option<InteractionResponder>,
) -> bool {
    match key.code {
        KeyCode::Char(c) if ('1'..='9').contains(&c) && !modal.is_searchable => {
            let digit = (c as u8 - b'1') as usize;
            if digit < modal.options.len() {
                modal.selected = digit;
            }
        }
        KeyCode::Backspace if modal.is_searchable => {
            let mut q = modal.filter_query.clone();
            q.pop();
            modal.set_filter(&q);
        }
        KeyCode::Char(c)
            if modal.is_searchable && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut q = modal.filter_query.clone();
            q.push(c);
            modal.set_filter(&q);
        }
        KeyCode::Enter => {
            let selected = modal.selected;
            if let Some(opt) = modal.options.get(selected)
                && let Some(input_spec) = opt.input.clone()
            {
                modal.input_option = Some(selected);
                modal.enter_input_mode(&input_spec.label);
                if let Some(val) = input_spec.value {
                    modal.input.set_text(val);
                }
                return false;
            }
            if let Some(resp) = active_responder.take() {
                let _ = resp.respond(InteractionResponse::Selected(selected));
            } else {
                apply_modal_selection(modal, session);
            }
            return true;
        }
        _ => {}
    }
    false
}

fn handle_modal_key(
    modal: &mut ModalState,
    key: KeyEvent,
    session: &mut ReplSession,
    active_responder: &mut Option<InteractionResponder>,
) -> bool {
    if key.code == KeyCode::Esc {
        if matches!(modal.mode, ModalMode::Input { .. }) && !modal.options.is_empty() {
            modal.exit_input_mode();
            return false;
        }
        if let Some(resp) = active_responder.take() {
            let _ = resp.respond(InteractionResponse::Cancelled);
        }
        return true;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        if !modal.filter_query.is_empty() {
            modal.set_filter("");
            return false;
        }
        if let Some(resp) = active_responder.take() {
            let _ = resp.respond(InteractionResponse::Cancelled);
        }
        return true;
    }

    if matches!(modal.mode, ModalMode::Input { .. }) {
        return handle_modal_input_mode(modal, key, active_responder);
    }

    handle_modal_nav_key(modal, key);
    handle_modal_action_key(modal, key, session, active_responder)
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
    pub context_window: usize,
    pub context_percent: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub quota: Option<String>,
}

fn make_footer_info(model: &str, provider: &str, thinking: Option<&str>, engine: &AgentEngine) -> FooterInfo {
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
    pub active_modal: &'a mut Option<ModalState>,
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
    let left = path;
    let right = footer.quota.as_deref().unwrap_or("");
    let plain_len = left.len() + right.len();
    let pad = width.saturating_sub(plain_len);
    format!("\x1b[90m{left}{}{right}\x1b[0m", " ".repeat(pad))
}

fn format_footer_stats(footer: &FooterInfo, width: usize) -> String {
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
            crate::ui::interactive::apply_software_cursor(&mut row, c_col);
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
        let widget_input = RunningToolWidgetInput {
            tool,
            theme: &state.session.renderer.theme,
            width,
            tools_expanded: state.session.config.ui.tools_expanded.unwrap_or(false),
        };
        let tool_lines = render_running_tool_widget(widget_input);
        let height = crate::ui::terminal_height() as usize;
        let budget = if state.session.config.ui.tools_expanded.unwrap_or(false) {
            ((height as f64) * 0.60).round() as usize
        } else {
            10
        };
        let windowed = crate::ui::interactive::window_widget_lines(&tool_lines, budget);
        lines.extend(windowed);
    }

    for steer in queued_steering {
        lines.push(format!("\x1b[36m↳ Steering: {steer}\x1b[0m"));
    }

    let (style, reset) = if state.active_modal.is_some() {
        ("\x1b[1;36m", "\x1b[0m")
    } else {
        thinking_divider_style(footer.thinking.as_deref())
    };

    let top_divider = match state.active_modal.as_ref() {
        Some(modal) => modal_top_divider(width, modal_banner_title(modal), style, reset),
        None => match activity {
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
        },
    };
    lines.push(top_divider);

    let (cursor_row, cursor_col) = if let Some(modal) = state.active_modal.as_ref() {
        let (modal_lines, cur_pos, _) = render_in_input_modal(InInputModalInput {
            modal,
            draft_text: &state.editor.text(),
            bounds: (width, 14),
            theme: &state.session.renderer.theme,
            focused: true,
        });
        let start_row = lines.len();
        lines.extend(modal_lines);
        (start_row + cur_pos.row, cur_pos.column)
    } else {
        render_live_editor_lines(state, &mut lines, cursor_mode)
    };

    lines.push(format!("{style}{}{reset}", "─".repeat(width)));
    if let Some(modal) = state.active_modal.as_ref() {
        let hint = modal_hint(modal);
        lines.push(format!("\x1b[90m{hint}\x1b[0m"));
    }
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

        state.tracker.restore_cursor(&mut stdout, width)?;
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
        active_editor: state.active_modal.is_none(),
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
        active_editor: state.active_modal.is_none(),
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
            *state.active_modal = Some(build_model_modal(state.session));
            Some(ActionOutcome::Handled)
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

async fn handle_key_cycle(
    state: &mut RunnerState<'_>,
    engine: &mut AgentEngine,
    key: KeyEvent,
    completions: &CompletionEngine,
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
) -> Result<bool> {
    if let Some(mut modal) = state.active_modal.take() {
        let closed = handle_modal_key(&mut modal, key, state.session, &mut None);
        if !closed {
            *state.active_modal = Some(modal);
        }
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
    if let Some(outcome) = handle_input_action(action, state, engine).await {
        return match outcome {
            ActionOutcome::Exit => Ok(true),
            ActionOutcome::FullRedraw => {
                let footer = make_footer_info(
                    &state.session.config.model,
                    &state.session.config.provider,
                    state.session.config.thinking_level.as_deref(),
                    engine,
                );
                full_redraw(state, &footer, None, &[])?;
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

    let mut active_modal: Option<ModalState> = None;
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
        transcript: &mut ctx.transcript,
        tracker: &mut tracker,
        prev_lines_count: 0,
        prev_cursor_row: 0,
    };

    let footer = make_footer_info(
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

fn build_settings_modal(session: &ReplSession) -> ModalState {
    let hide_thinking = session.config.ui.hide_thinking.unwrap_or(false);
    let tools_expanded = session.config.ui.tools_expanded.unwrap_or(true);
    let vim_mode = session.config.editor.is_vim();
    let options = vec![
        ModalOption::new(
            "Hide Thinking",
            Some(if hide_thinking { "hidden ✓" } else { "visible" }.to_string()),
        ),
        ModalOption::new(
            "Expand Tools",
            Some(if tools_expanded { "expanded ✓" } else { "collapsed" }.to_string()),
        ),
        ModalOption::new(
            "Vim Mode",
            Some(if vim_mode { "enabled ✓" } else { "disabled" }.to_string()),
        ),
    ];
    ModalState::new("Settings", "", options)
}

fn build_model_modal(session: &ReplSession) -> ModalState {
    let discovered = crate::repl::interactive::discover_models(&session.config, &session.auth_store);
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, item) in discovered.iter().enumerate() {
        if item.id == session.config.model {
            initial_selection = i;
        }
        let active_mark = if item.id == session.config.model { "✓" } else { "" };
        let default_mark = if session.config.default_model.as_deref().is_some_and(|dm| dm == item.id) {
            "default"
        } else {
            ""
        };
        options.push(ModalOption::new(
            item.id.clone(),
            Some(format!(
                "{}\t{}\t{}\t{}",
                item.provider, active_mark, default_mark, item.description
            )),
        ));
    }

    let mut modal = ModalState::new("Select Model", "", options).with_search(true);
    modal.selected = initial_selection;
    modal
}

fn build_thinking_modal(current_thinking: Option<&str>) -> ModalState {
    let active = current_thinking.unwrap_or("off");
    let mut options = Vec::new();
    let mut initial_selection = 0;

    for (i, (level, desc)) in rho_ui_core::autocomplete::THINKING_LEVEL_OPTIONS.iter().enumerate() {
        let is_active = *level == active;
        if is_active {
            initial_selection = i;
        }
        let check = if is_active { "✓" } else { "" };
        options.push(ModalOption::new(
            format!("{level:<10}"),
            Some(format!("\t{check}\t\t{desc}")),
        ));
    }

    let mut modal = ModalState::new("Select Thinking Level", "", options);
    modal.selected = initial_selection;
    modal
}

fn build_session_modal(summaries: &[rho_harness_core::session::SessionSummary], active_id: Option<&str>) -> ModalState {
    let options = summaries
        .iter()
        .map(|s| {
            let is_active = active_id.is_some_and(|aid| aid == s.session_id);
            let active_mark = if is_active { "✓" } else { "" };
            let time = rho_ui_core::format_relative_time(s.last_modified);
            let title = s.name.clone().unwrap_or_else(|| s.session_id.clone());
            ModalOption::new(title, Some(format!("{}\t{}\t{}", s.session_id, active_mark, time)))
        })
        .collect();
    ModalState::new("Resume Session", "", options).with_search(true)
}

fn build_mcp_modal(session: &ReplSession) -> ModalState {
    let statuses = rho_engine::mcp::get_mcp_server_statuses();
    let options = session
        .config
        .mcp
        .servers
        .iter()
        .map(|(name, cfg)| {
            let (status_text, is_active) = if let Some(st) = statuses.get(name) {
                if let Some(err) = &st.error {
                    (format!("failed: {err}"), false)
                } else if st.is_loaded {
                    (format!("active ({} tools)", st.tools_count), true)
                } else {
                    ("disabled".to_string(), false)
                }
            } else if cfg.enabled {
                ("active".to_string(), true)
            } else {
                ("disabled".to_string(), false)
            };
            let check = if is_active { "✓" } else { "" };
            let command = cfg.command.as_deref().or(cfg.url.as_deref()).unwrap_or("");
            ModalOption::new(name.clone(), Some(format!("{}\t{}\t{}", command, check, status_text)))
        })
        .collect();
    ModalState::new("Model Context Protocol", "", options)
}

fn build_skill_modal() -> ModalState {
    let skills = crate::skills::resolved_skills(std::env::current_dir().ok().as_deref());
    let options = skills
        .into_iter()
        .map(|s| ModalOption::new(s.metadata.name, Some(s.metadata.description)))
        .collect();
    ModalState::new("Skills", "", options).with_search(true)
}

async fn handle_interactive_command(
    cmd: &str,
    session: &mut ReplSession,
    engine: &AgentEngine,
    active_modal: &mut Option<ModalState>,
) -> bool {
    match cmd {
        "/model" => {
            *active_modal = Some(build_model_modal(session));
            true
        }
        "/thinking" => {
            *active_modal = Some(build_thinking_modal(session.config.thinking_level.as_deref()));
            true
        }
        "/settings" => {
            *active_modal = Some(build_settings_modal(session));
            true
        }
        "/session" => {
            let summaries = list_session_summaries_async(&session.config.sessions_dir)
                .await
                .unwrap_or_default();
            *active_modal = Some(build_session_modal(
                &summaries,
                Some(&engine.session_manager.session_id),
            ));
            true
        }
        "/mcp" => {
            *active_modal = Some(build_mcp_modal(session));
            true
        }
        "/skill" => {
            *active_modal = Some(build_skill_modal());
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
    active_responder: &mut Option<InteractionResponder>,
) -> TurnKeyOutcome {
    if let Some(mut modal) = state.active_modal.take() {
        let closed = handle_modal_key(&mut modal, key, state.session, active_responder);
        if !closed {
            *state.active_modal = Some(modal);
        }
        return TurnKeyOutcome::Handled;
    }
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
    pub responder: Option<InteractionResponder>,
    pub tick_counter: usize,
    pub spinner_frame: usize,
}

fn handle_turn_event(ui_ev: UiEvent, state: &mut RunnerState<'_>, stream: &mut TurnStreamState) {
    match ui_ev {
        UiEvent::Interaction { prompt: p, responder } => {
            let modal = build_interaction_state(p);
            *state.active_modal = Some(modal);
            stream.responder = Some(responder);
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

fn handle_turn_tick(
    state: &mut RunnerState<'_>,
    footer: &FooterInfo,
    stream: &mut TurnStreamState,
    steering: &SharedSteeringQueue,
) -> Result<()> {
    stream.tick_counter += 1;
    if stream.tick_counter.is_multiple_of(5) {
        stream.spinner_frame = (stream.spinner_frame + 1) % 10;
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
    let outcome = handle_turn_key_input(key, state, steering, cancellation, &mut stream.responder);
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
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
) -> Result<()> {
    for msg in unconsumed {
        Box::pin(execute_agent_turn(state, engine, &msg, ui_events, events)).await?;
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
    ui_events: &mut tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
    events: &mut EventStream,
) -> Result<()> {
    while ui_events.try_recv().is_ok() {}

    state.transcript.push(TranscriptItem::UserMessage(prompt.to_string()));
    state.tracker.clear();

    let footer = make_footer_info(
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
                while let Ok(ui_ev) = ui_events.try_recv() {
                    drain_ui_event(ui_ev, &mut stream.scrollback, &mut stream.activity, &mut stream.running_tool, state.transcript, state.session);
                }
                finalize_turn_result(res, state, &footer, &mut stream)?;
                break;
            }
            Some(ui_ev) = ui_events.recv() => {
                handle_turn_event(ui_ev, state, &mut stream);
            }
            _ = ticker.tick() => {
                handle_turn_tick(state, &footer, &mut stream, &steering)?;
            }
            maybe_key = events.next() => {
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
    *state.active_modal = None;

    finish_turn_execution(state, engine).await?;
    let unconsumed = steering.current_items();
    if !unconsumed.is_empty() && !is_cancelled {
        steering.clear();
        dispatch_unconsumed_steering(unconsumed, state, engine, ui_events, events).await?;
    }
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
