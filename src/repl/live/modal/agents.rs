use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::agents::{AgentEvent, AgentMonitor, AgentRecord, AgentStatus, AgentSummary};
use std::time::Duration;

use crate::error::Result;
use crate::ui::interactive::{
    BodyAnchor, ModalOption, ModalState, TerminalBackend, TerminalController, TranscriptItem,
};

pub const AGENTS_TITLE: &str = "Agents";
const AGENT_VIEW_PREFIX: &str = "Agent: ";
const MESSAGE_PROMPT: &str = "message";
const ARGS_PREVIEW_CHARS: usize = 160;
const PAGE_LINES: usize = 10;
/// Columns the modal frame takes from the terminal width.
const MODAL_FRAME_COLUMNS: usize = 4;

/// A message the user typed in an agent view, for the agent's saved task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessage {
    pub task_id: String,
    pub label: String,
    /// The agent's round was still running when the message was sent.
    pub running: bool,
    pub text: String,
}

/// What a key did in the agent list or view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKey {
    /// Neither is open; the key belongs to someone else.
    Ignored,
    Handled,
    Send(AgentMessage),
    /// Stop the running round of this task.
    Stop {
        task_id: String,
        label: String,
    },
}

impl AgentKey {
    #[must_use]
    pub fn consumed(&self) -> bool {
        *self != Self::Ignored
    }
}

/// Where an agent-view message goes: straight to the running round, or, once the round has ended,
/// to the main agent as a request to continue the task (so it runs inside a permission-checked turn).
#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentDelivery {
    Sent,
    ContinueThroughMainAgent(String),
}

/// Applies a key's outcome that needs the engine: messages and stop requests. Returns the prompt
/// to hand the main agent when a finished agent's task should be continued.
pub fn apply_agent_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    engine: &crate::engine::AgentEngine,
    outcome: &AgentKey,
) -> Option<String> {
    match outcome {
        AgentKey::Send(message) => match deliver_agent_message(controller, engine, message) {
            AgentDelivery::Sent => None,
            AgentDelivery::ContinueThroughMainAgent(prompt) => Some(prompt),
        },
        AgentKey::Stop { task_id, label } => {
            let status = if engine.stop_subagent(task_id) {
                format!("Stopping {label}; it saves its work first")
            } else {
                format!("{label} is not running")
            };
            controller.set_system_message(status);
            None
        }
        AgentKey::Ignored | AgentKey::Handled => None,
    }
}

/// Delivers `message`, returning where it went and setting a status line saying so.
fn deliver_agent_message<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    engine: &crate::engine::AgentEngine,
    message: &AgentMessage,
) -> AgentDelivery {
    if message.running && engine.message_running_subagent(&message.task_id, &message.text) {
        controller.set_system_message(format!(
            "Sent to {}; it reads it before its next model call",
            message.label
        ));
        return AgentDelivery::Sent;
    }
    controller.set_system_message(format!("Asking the main agent to continue {}", message.label));
    AgentDelivery::ContinueThroughMainAgent(continue_prompt(message))
}

fn continue_prompt(message: &AgentMessage) -> String {
    format!(
        "Continue subagent task `{}` ({}) with the subagent tool, passing it my message below as the prompt, then report what it did.\n\n{}",
        message.task_id, message.label, message.text
    )
}

fn is_agent_view(modal: &ModalState) -> bool {
    modal.title.starts_with(AGENT_VIEW_PREFIX)
}

fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

fn status_word(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => "running",
        AgentStatus::Done => "done",
        AgentStatus::InputRequired => "needs input",
        AgentStatus::Failed => "failed",
        AgentStatus::Cancelled => "cancelled",
    }
}

fn progress(summary: &AgentSummary) -> String {
    match summary.status {
        AgentStatus::Running => format!("turn {}/{}", summary.turn, summary.max_turns),
        status => format!("{} after {} turns", status_word(status), summary.turn),
    }
}

fn list_options(monitor: &AgentMonitor) -> Vec<ModalOption> {
    let mut summaries = monitor.summaries();
    summaries.sort_by_key(|s| (s.status != AgentStatus::Running, std::cmp::Reverse(s.id)));
    summaries
        .iter()
        .map(|s| {
            let label: String = s.label.chars().take(28).collect();
            let description = format!(
                "{} · {} · {} · {}",
                s.group,
                s.model,
                progress(s),
                format_elapsed(s.elapsed)
            );
            ModalOption::new(format!("{} {label:<28}", s.status.symbol()), Some(description))
                .with_value(s.id.to_string())
        })
        .collect()
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let kept: String = flat.chars().take(max).collect();
    format!("{kept}…")
}

fn status_line(record: &AgentRecord) -> String {
    let s = &record.summary;
    let mut line = format!(
        "{} {} · {} · {} · {} · {}",
        s.status.symbol(),
        status_word(s.status),
        s.model,
        progress(s),
        format_elapsed(s.elapsed),
        s.group
    );
    if let Some(task_id) = &s.task_id {
        line.push_str(&format!(" · task {task_id}"));
    }
    line
}

/// The agent's record as the items the main transcript is made of: its prompts and messages as user
/// blocks, its replies as assistant text, finished tools as tool cards, and running tools as notices.
pub fn agent_items(record: &AgentRecord) -> Vec<TranscriptItem> {
    let mut items = vec![TranscriptItem::Notice(status_line(record))];
    if record.dropped_events > 0 {
        items.push(TranscriptItem::Notice(format!(
            "({} earlier events not kept)",
            record.dropped_events
        )));
    }
    items.extend(record.events.iter().map(|event| match event {
        AgentEvent::Task(text) => TranscriptItem::UserMessage(text.trim().to_string()),
        AgentEvent::Text(text) => TranscriptItem::AssistantText(text.clone()),
        AgentEvent::ToolCall { name, args } => {
            TranscriptItem::Notice(format!("⟳ {name} {}", one_line(args, ARGS_PREVIEW_CHARS)))
        }
        AgentEvent::Tool(line) => TranscriptItem::Tool(line.clone()),
    }));
    items
}

/// The agent view body: its transcript items rendered like the main transcript at the modal's width.
fn render_agent_body<B: TerminalBackend>(controller: &TerminalController<B>, record: &AgentRecord) -> String {
    let width = controller.terminal_width().saturating_sub(MODAL_FRAME_COLUMNS);
    agent_items(record)
        .iter()
        .map(|item| controller.render_detached(item, width).trim_matches('\n').to_string())
        .filter(|rendered| !rendered.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn agent_view<B: TerminalBackend>(controller: &TerminalController<B>, record: &AgentRecord) -> ModalState {
    let mut view = ModalState::new(
        format!("{AGENT_VIEW_PREFIX}{}", record.summary.label),
        render_agent_body(controller, record),
        Vec::new(),
    )
    .with_body_anchor(BodyAnchor::Bottom)
    .with_subject(record.summary.id.to_string())
    .with_full_screen();
    view.enter_input_mode(MESSAGE_PROMPT);
    view
}

fn open_selected<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) {
    let id = controller
        .state()
        .active_modal()
        .and_then(|m| m.selected_option())
        .and_then(|o| o.value.as_deref())
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(record) = id.and_then(|id| monitor.record(id)) {
        let view = agent_view(controller, &record);
        controller.state_mut().push_modal(view);
    }
}

/// Opens the agent list for Left on an empty prompt; returns whether the key was consumed.
pub fn try_open_agents<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: &KeyEvent,
    monitor: &AgentMonitor,
) -> Result<bool> {
    let state = controller.state();
    if key.code != KeyCode::Left
        || !key.modifiers.is_empty()
        || state.active_modal().is_some()
        || !state.editor().is_empty()
    {
        return Ok(false);
    }
    if monitor.is_empty() {
        controller.set_system_message("No subagents yet — they are listed here once they start");
    } else {
        let modal = ModalState::new(AGENTS_TITLE, "", list_options(monitor)).with_search(true);
        controller.state_mut().push_modal(modal);
    }
    controller.redraw()?;
    Ok(true)
}

/// Handles keys for the agent list and agent view.
pub fn handle_agents_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> Result<AgentKey> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(AgentKey::Ignored);
    };
    let outcome = if active.title == AGENTS_TITLE {
        handle_list_key(controller, key, monitor)?
    } else if is_agent_view(active) {
        handle_view_key(controller, key, monitor)
    } else {
        return Ok(AgentKey::Ignored);
    };
    controller.redraw()?;
    Ok(outcome)
}

fn handle_list_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> Result<AgentKey> {
    match key.code {
        KeyCode::Enter | KeyCode::Right => open_selected(controller, monitor),
        KeyCode::Left | KeyCode::Esc => {
            controller.state_mut().pop_modal();
        }
        KeyCode::Delete => return Ok(stop_selected(controller, monitor)),
        _ => {
            crate::repl::live::modal::handle_selector_nav(controller, &key)?;
        }
    }
    Ok(AgentKey::Handled)
}

/// A stop request for the selected agent if its round is running and saved.
fn stop_selected<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) -> AgentKey {
    let summary = controller
        .state()
        .active_modal()
        .and_then(|m| m.selected_option())
        .and_then(|o| o.value.as_deref())
        .and_then(|v| v.parse::<u64>().ok())
        .and_then(|id| monitor.record(id))
        .map(|record| record.summary);
    match summary {
        Some(s) if s.status == AgentStatus::Running => match s.task_id {
            Some(task_id) => AgentKey::Stop {
                task_id,
                label: s.label,
            },
            None => {
                controller.set_system_message("This agent's round is not saved; Esc stops it with the turn");
                AgentKey::Handled
            }
        },
        Some(s) => {
            controller.set_system_message(format!("{} is not running", s.label));
            AgentKey::Handled
        }
        None => AgentKey::Handled,
    }
}

fn scroll_by(modal: &mut ModalState, up: bool, lines: usize) {
    modal.body_scroll = if up {
        modal.body_scroll.saturating_add(lines).min(modal.body.lines().count())
    } else {
        modal.body_scroll.saturating_sub(lines)
    };
}

fn viewed_record<B: TerminalBackend>(
    controller: &TerminalController<B>,
    monitor: &AgentMonitor,
) -> Option<AgentRecord> {
    controller
        .state()
        .active_modal()
        .and_then(|modal| modal.subject.as_deref())
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(|id| monitor.record(id))
}

fn view_input(controller: &TerminalController<impl TerminalBackend>) -> String {
    controller
        .state()
        .active_modal()
        .map(|modal| modal.input.text().to_string())
        .unwrap_or_default()
}

fn edit_view_input<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    action: crate::ui::interactive::UiAction,
) {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        crate::repl::live::modal::apply_input_edit(&mut modal.input, action);
    }
}

/// Takes the typed message for the viewed agent, or explains why it cannot be messaged.
fn send_from_view<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) -> AgentKey {
    let text = view_input(controller).trim().to_string();
    if text.is_empty() {
        return AgentKey::Handled;
    }
    let Some(record) = viewed_record(controller, monitor) else {
        return AgentKey::Handled;
    };
    let Some(task_id) = record.summary.task_id.clone() else {
        controller.set_system_message("This agent's round was not saved, so it cannot be messaged");
        return AgentKey::Handled;
    };
    edit_view_input_clear(controller);
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.body_scroll = 0;
    }
    AgentKey::Send(AgentMessage {
        task_id,
        label: record.summary.label.clone(),
        running: record.summary.status == AgentStatus::Running,
        text,
    })
}

fn handle_view_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> AgentKey {
    use crate::ui::interactive::UiAction;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let typing = !view_input(controller).is_empty();
    match key.code {
        KeyCode::Char('c') if ctrl && typing => edit_view_input_clear(controller),
        KeyCode::Char('c') if ctrl => pop_view(controller),
        KeyCode::Esc => pop_view(controller),
        KeyCode::Left if !typing => pop_view(controller),
        KeyCode::Enter => return send_from_view(controller, monitor),
        KeyCode::Tab => switch_sibling(controller, monitor, false),
        KeyCode::BackTab => switch_sibling(controller, monitor, true),
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
            edit_view_input(controller, UiAction::Insert(c));
        }
        KeyCode::Backspace => edit_view_input(controller, UiAction::Backspace),
        KeyCode::Delete => edit_view_input(controller, UiAction::Delete),
        KeyCode::Left => edit_view_input(controller, UiAction::MoveLeft),
        KeyCode::Right => edit_view_input(controller, UiAction::MoveRight),
        KeyCode::Home => edit_view_input(controller, UiAction::MoveToStart),
        code => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                scroll_view(modal, code);
            }
        }
    }
    AgentKey::Handled
}

fn edit_view_input_clear<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    if let Some(modal) = controller.state_mut().active_modal_mut() {
        modal.input.set_text("");
    }
}

fn pop_view<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    controller.state_mut().pop_modal();
}

fn scroll_view(modal: &mut ModalState, code: KeyCode) {
    match code {
        KeyCode::Up => scroll_by(modal, true, 1),
        KeyCode::Down => scroll_by(modal, false, 1),
        KeyCode::PageUp => scroll_by(modal, true, PAGE_LINES),
        KeyCode::PageDown => scroll_by(modal, false, PAGE_LINES),
        KeyCode::End => modal.body_scroll = 0,
        _ => {}
    }
}

fn switch_sibling<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor, previous: bool) {
    let below_is_list = controller.state().modal_depth() >= 2;
    controller.state_mut().pop_modal();
    let on_list = controller
        .state()
        .active_modal()
        .is_some_and(|m| m.title == AGENTS_TITLE);
    if !(below_is_list && on_list) {
        return;
    }
    if previous {
        controller.state_mut().select_previous_modal_option();
    } else {
        controller.state_mut().select_next_modal_option();
    }
    open_selected(controller, monitor);
}

fn refresh_list(modal: &mut ModalState, monitor: &AgentMonitor) -> bool {
    let options = list_options(monitor);
    if options == modal.all_options {
        return false;
    }
    let selected = modal.selected_option().and_then(|o| o.value.clone());
    let query = modal.filter_query.clone();
    modal.all_options = options;
    modal.set_filter(&query);
    if let Some(index) = modal.options.iter().position(|o| o.value == selected) {
        modal.selected = index;
    }
    true
}

/// The newest round of the same agent (same group and label) started after `record`, if any: a view
/// follows the agent into the rounds that continue its task.
fn newer_round(monitor: &AgentMonitor, record: &AgentRecord) -> Option<AgentRecord> {
    monitor
        .summaries()
        .into_iter()
        .filter(|s| s.id > record.summary.id && s.group == record.summary.group && s.label == record.summary.label)
        .max_by_key(|s| s.id)
        .and_then(|s| monitor.record(s.id))
}

fn refresh_view<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) -> bool {
    let Some(mut record) = viewed_record(controller, monitor) else {
        return false;
    };
    let mut followed = false;
    if let Some(newer) = newer_round(monitor, &record) {
        record = newer;
        followed = true;
    }
    let body = render_agent_body(controller, &record);
    let Some(modal) = controller.state_mut().active_modal_mut() else {
        return false;
    };
    if followed {
        modal.subject = Some(record.summary.id.to_string());
        modal.body_scroll = 0;
    }
    if body == modal.body {
        return followed;
    }
    if modal.body_scroll > 0 {
        let added = body.lines().count().saturating_sub(modal.body.lines().count());
        modal.body_scroll += added;
    }
    modal.body = body;
    true
}

/// Re-renders an open agent list or view from the monitor and updates the footer's running-agent
/// count; returns whether anything changed.
pub fn refresh_agent_modals<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    monitor: &AgentMonitor,
) -> bool {
    let running = monitor
        .summaries()
        .iter()
        .filter(|s| s.status == AgentStatus::Running)
        .count();
    let count_changed = controller.state().footer().running_agents != running;
    if count_changed {
        controller.state_mut().footer_mut().running_agents = running;
    }
    let Some(modal) = controller.state_mut().active_modal_mut() else {
        return count_changed;
    };
    let view_changed = if modal.title == AGENTS_TITLE {
        refresh_list(modal, monitor)
    } else if is_agent_view(modal) {
        refresh_view(controller, monitor)
    } else {
        false
    };
    count_changed || view_changed
}

#[cfg(test)]
mod tests;
