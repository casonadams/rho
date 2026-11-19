use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::agents::{AgentEvent, AgentMonitor, AgentRecord, AgentStatus, AgentSummary};
use std::time::Duration;

use crate::error::Result;
use crate::ui::interactive::{BodyAnchor, ModalOption, ModalState, TerminalBackend, TerminalController};

pub const AGENTS_TITLE: &str = "Agents";
const AGENT_VIEW_PREFIX: &str = "Agent: ";
const TASK_PREVIEW_LINES: usize = 12;
const OUTPUT_PREVIEW_LINES: usize = 4;
const ARGS_PREVIEW_CHARS: usize = 160;
const PAGE_LINES: usize = 10;

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
        AgentStatus::Incomplete => "incomplete",
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

fn preview_lines(text: &str, max: usize, indent: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let mut out: Vec<String> = lines.iter().take(max).map(|l| format!("{indent}{l}")).collect();
    if lines.len() > max {
        out.push(format!("{indent}… {} more lines", lines.len() - max));
    }
    out.join("\n")
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let kept: String = flat.chars().take(max).collect();
    format!("{kept}…")
}

fn render_event(event: &AgentEvent) -> String {
    match event {
        AgentEvent::Task(text) => format!("▍Task\n{}", preview_lines(text, TASK_PREVIEW_LINES, "  ")),
        AgentEvent::Text(text) => format!("● {}", text.trim()),
        AgentEvent::ToolCall { name, args } => format!("→ {name} {}", one_line(args, ARGS_PREVIEW_CHARS)),
        AgentEvent::ToolResult { name, output, is_error } => {
            let mark = if *is_error { "✗" } else { "←" };
            format!(
                "  {mark} {name}\n{}",
                preview_lines(output, OUTPUT_PREVIEW_LINES, "    ")
            )
        }
    }
}

pub fn render_agent_body(record: &AgentRecord) -> String {
    let s = &record.summary;
    let mut sections = vec![format!(
        "{} {} · {} · {} · {} · {}",
        s.status.symbol(),
        status_word(s.status),
        s.model,
        progress(s),
        format_elapsed(s.elapsed),
        s.group
    )];
    if record.dropped_events > 0 {
        sections.push(format!("({} earlier events not kept)", record.dropped_events));
    }
    sections.extend(record.events.iter().map(render_event));
    sections.join("\n\n")
}

fn agent_view(record: &AgentRecord) -> ModalState {
    ModalState::new(
        format!("{AGENT_VIEW_PREFIX}{}", record.summary.label),
        render_agent_body(record),
        Vec::new(),
    )
    .with_body_anchor(BodyAnchor::Bottom)
    .with_subject(record.summary.id.to_string())
}

fn open_selected<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) {
    let id = controller
        .state()
        .active_modal()
        .and_then(|m| m.selected_option())
        .and_then(|o| o.value.as_deref())
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(record) = id.and_then(|id| monitor.record(id)) {
        controller.state_mut().push_modal(agent_view(&record));
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

/// Handles keys for the agent list and agent view; returns false when neither is active.
pub fn handle_agents_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> Result<bool> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(false);
    };
    if active.title == AGENTS_TITLE {
        handle_list_key(controller, key, monitor)?;
    } else if is_agent_view(active) {
        handle_view_key(controller, key, monitor);
    } else {
        return Ok(false);
    }
    controller.redraw()?;
    Ok(true)
}

fn handle_list_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> Result<()> {
    match key.code {
        KeyCode::Enter | KeyCode::Right => open_selected(controller, monitor),
        KeyCode::Left | KeyCode::Esc => {
            controller.state_mut().pop_modal();
        }
        _ => {
            crate::repl::live::modal::handle_selector_nav(controller, &key)?;
        }
    }
    Ok(())
}

fn scroll_by(modal: &mut ModalState, up: bool, lines: usize) {
    modal.body_scroll = if up {
        modal.body_scroll.saturating_add(lines).min(modal.body.lines().count())
    } else {
        modal.body_scroll.saturating_sub(lines)
    };
}

fn handle_view_key<B: TerminalBackend>(controller: &mut TerminalController<B>, key: KeyEvent, monitor: &AgentMonitor) {
    let ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Left | KeyCode::Esc => {
            controller.state_mut().pop_modal();
        }
        _ if ctrl_c => {
            controller.state_mut().pop_modal();
        }
        KeyCode::Tab => switch_sibling(controller, monitor, false),
        KeyCode::BackTab => switch_sibling(controller, monitor, true),
        code => {
            if let Some(modal) = controller.state_mut().active_modal_mut() {
                scroll_view(modal, code);
            }
        }
    }
}

fn scroll_view(modal: &mut ModalState, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => scroll_by(modal, true, 1),
        KeyCode::Down | KeyCode::Char('j') => scroll_by(modal, false, 1),
        KeyCode::PageUp => scroll_by(modal, true, PAGE_LINES),
        KeyCode::PageDown => scroll_by(modal, false, PAGE_LINES),
        KeyCode::End | KeyCode::Char('G') => modal.body_scroll = 0,
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

fn refresh_view(modal: &mut ModalState, monitor: &AgentMonitor) -> bool {
    let record = modal
        .subject
        .as_deref()
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(|id| monitor.record(id));
    let Some(record) = record else {
        return false;
    };
    let body = render_agent_body(&record);
    if body == modal.body {
        return false;
    }
    if modal.body_scroll > 0 {
        let added = body.lines().count().saturating_sub(modal.body.lines().count());
        modal.body_scroll += added;
    }
    modal.body = body;
    true
}

/// Re-renders an open agent list or view from the monitor; returns whether anything changed.
pub fn refresh_agent_modals<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    monitor: &AgentMonitor,
) -> bool {
    let Some(modal) = controller.state_mut().active_modal_mut() else {
        return false;
    };
    if modal.title == AGENTS_TITLE {
        refresh_list(modal, monitor)
    } else if is_agent_view(modal) {
        refresh_view(modal, monitor)
    } else {
        false
    }
}

#[cfg(test)]
mod tests;
