use crossterm::event::{KeyCode, KeyEvent};
use rho_harness_core::agents::{AgentMonitor, AgentStatus, AgentSummary};
use std::time::Duration;

use crate::error::Result;
use crate::repl::live::agent_focus;
use crate::ui::interactive::{ModalOption, ModalState, TerminalBackend, TerminalController};

pub const AGENTS_TITLE: &str = "Agents";

/// A running subagent round to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopRequest {
    pub task_id: String,
    pub label: String,
}

/// What a key did in the agent list or while an agent is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentKey {
    /// Neither applies; the key belongs to someone else.
    Ignored,
    Handled,
    /// Stop the running round of this task.
    Stop(StopRequest),
    /// `Esc` while an agent is shown: stop it when it can be stopped, otherwise return to the main agent.
    Escape(Option<StopRequest>),
}

impl AgentKey {
    #[must_use]
    pub fn consumed(&self) -> bool {
        *self != Self::Ignored
    }
}

/// Applies a key's outcome that needs the engine: stopping an agent, or leaving its view.
pub fn apply_agent_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    engine: &crate::engine::AgentEngine,
    outcome: &AgentKey,
) -> Result<()> {
    match outcome {
        AgentKey::Stop(request) => {
            let status = if engine.stop_subagent(&request.task_id) {
                format!("Stopping {}; it saves its work first", request.label)
            } else {
                format!("{} is not running", request.label)
            };
            controller.set_system_message(status);
        }
        AgentKey::Escape(Some(request)) if engine.stop_subagent(&request.task_id) => {
            controller.set_system_message(format!("Stopping {}; it saves its work first", request.label));
        }
        AgentKey::Escape(_) => controller.leave_agent_view()?,
        AgentKey::Ignored | AgentKey::Handled => {}
    }
    Ok(())
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

/// How far the agent is: its turn while running, otherwise how it ended.
pub(crate) fn progress(summary: &AgentSummary) -> String {
    let base = match summary.status {
        AgentStatus::Running => format!("turn {}/{}", summary.turn, summary.max_turns),
        status => format!("{} after {} turns", status_word(status), summary.turn),
    };
    if summary.awaiting_approval {
        format!("{base} \u{b7} needs approval")
    } else {
        base
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
                "{} \u{b7} {} \u{b7} {} \u{b7} {}",
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

fn selected_agent<B: TerminalBackend>(controller: &TerminalController<B>) -> Option<u64> {
    controller
        .state()
        .active_modal()
        .and_then(|m| m.selected_option())
        .and_then(|o| o.value.as_deref())
        .and_then(|v| v.parse::<u64>().ok())
}

/// Closes the list and shows the selected agent in the main terminal.
fn open_selected<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) -> Result<()> {
    let Some(id) = selected_agent(controller) else {
        return Ok(());
    };
    controller.state_mut().pop_modal();
    agent_focus::show_agent(controller, Some(monitor), Some(id))
}

/// `Left` on an empty prompt: returns to the main agent while another agent is shown, otherwise
/// opens the agent list. Returns whether the key was consumed.
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
    if controller.viewed_agent().is_some() {
        controller.leave_agent_view()?;
        return Ok(true);
    }
    if monitor.is_empty() {
        controller.set_system_message("No subagents yet \u{2014} they are listed here once they start");
    } else {
        let modal = ModalState::new(AGENTS_TITLE, "", list_options(monitor)).with_search(true);
        controller.state_mut().push_modal(modal);
    }
    controller.redraw()?;
    Ok(true)
}

/// Handles keys for the agent list, and `Esc` while an agent is shown.
pub fn handle_agents_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> Result<AgentKey> {
    let Some(active) = controller.state().active_modal() else {
        return Ok(escape_from_view(controller, &key, monitor));
    };
    if active.title != AGENTS_TITLE {
        return Ok(AgentKey::Ignored);
    }
    let outcome = handle_list_key(controller, key, monitor)?;
    controller.redraw()?;
    Ok(outcome)
}

fn escape_from_view<B: TerminalBackend>(
    controller: &TerminalController<B>,
    key: &KeyEvent,
    monitor: &AgentMonitor,
) -> AgentKey {
    let Some(id) = controller.viewed_agent() else {
        return AgentKey::Ignored;
    };
    if key.code != KeyCode::Esc || !key.modifiers.is_empty() {
        return AgentKey::Ignored;
    }
    let stop = monitor
        .record(id)
        .map(|record| record.summary)
        .filter(|s| s.status == AgentStatus::Running)
        .and_then(|s| {
            Some(StopRequest {
                task_id: s.task_id?,
                label: s.label,
            })
        });
    AgentKey::Escape(stop)
}

fn handle_list_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
    monitor: &AgentMonitor,
) -> Result<AgentKey> {
    match key.code {
        KeyCode::Enter | KeyCode::Right => open_selected(controller, monitor)?,
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
    let summary = selected_agent(controller)
        .and_then(|id| monitor.record(id))
        .map(|record| record.summary);
    match summary {
        Some(s) if s.status == AgentStatus::Running => match s.task_id {
            Some(task_id) => AgentKey::Stop(StopRequest {
                task_id,
                label: s.label,
            }),
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

/// Brings the agent list, the shown agent, and the footer's running-agent count up to date;
/// returns whether anything changed.
pub fn refresh_agent_modals<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    monitor: &AgentMonitor,
) -> Result<bool> {
    let running = monitor
        .summaries()
        .iter()
        .filter(|s| s.status == AgentStatus::Running)
        .count();
    let count_changed = controller.state().footer().running_agents != running;
    if count_changed {
        controller.state_mut().footer_mut().running_agents = running;
    }
    let view_changed = agent_focus::refresh(controller, monitor)?;
    let list_changed = controller
        .state_mut()
        .active_modal_mut()
        .filter(|modal| modal.title == AGENTS_TITLE)
        .is_some_and(|modal| refresh_list(modal, monitor));
    Ok(count_changed || view_changed || list_changed)
}

#[cfg(test)]
mod tests;
