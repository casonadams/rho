//! Shows a subagent in the terminal in place of the main agent, the way the main agent is shown:
//! its prompts, streamed reply, thinking, and tool cards, with the footer naming it. Text typed
//! while it is shown goes to it. `Left` on an empty prompt returns to the main agent.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::agents::{AgentEvent, AgentMonitor, AgentRecord, AgentStatus};
use rho_harness_core::presentation::summary::format_tool_args_summary;

use crate::error::Result;
use crate::repl::live::modal::agents::progress;
use crate::ui::interactive::{
    RunningTool, TerminalBackend, TerminalController, ToolStartRequest, TranscriptItem, ViewFrame,
};

/// The agent's record as the main terminal shows it: settled items, the reply it is still
/// writing, and the tool it is running (as a live card, like the main agent's).
pub fn frame(record: &AgentRecord) -> ViewFrame {
    let running = record.summary.status == AgentStatus::Running;
    let mut frame = ViewFrame::default();
    if record.dropped_events > 0 {
        frame.items.push(TranscriptItem::Notice(format!(
            "({} earlier events not kept)",
            record.dropped_events
        )));
    }
    for event in &record.events {
        match event {
            AgentEvent::Task(text) => frame.items.push(TranscriptItem::UserMessage(text.trim().to_string())),
            AgentEvent::Text(text) => frame.items.push(TranscriptItem::AssistantText(text.clone())),
            AgentEvent::Thinking(text) => frame.items.push(TranscriptItem::Thinking(text.clone())),
            AgentEvent::Tool(line) if RunningTool::has_transcript_card(&line.name) => {
                frame.items.push(TranscriptItem::Tool(line.clone()));
            }
            AgentEvent::Tool(_) => {}
            AgentEvent::ToolCall {
                name,
                args,
                output,
                dropped,
            } if running => {
                frame.running_tool = live_card(name, args);
                if frame.running_tool.is_some() {
                    frame.running_output = output.clone();
                    frame.running_output_start = *dropped;
                } else {
                    frame.running_output = String::new();
                    frame.running_output_start = 0;
                }
            }
            AgentEvent::ToolCall { name, .. } => {
                frame
                    .items
                    .push(TranscriptItem::Notice(format!("\u{2717} {name} did not finish")));
            }
        }
    }
    if running {
        match record.events.last() {
            Some(AgentEvent::Text(text)) => {
                frame.items.pop();
                frame.streaming = Some(text.clone());
            }
            Some(AgentEvent::Thinking(_)) => {
                frame.items.pop();
            }
            _ => {}
        }
    }
    frame
}

/// A running tool's card, for the tools the main agent shows one for.
fn live_card(name: &str, args: &serde_json::Value) -> Option<ToolStartRequest> {
    if !RunningTool::has_live_card(name) {
        return None;
    }
    Some(ToolStartRequest {
        name: name.to_string(),
        args_summary: format_tool_args_summary(name, args),
        arguments: args.clone(),
    })
}

/// The footer title of the shown agent: who it is, its model, and how far it is.
fn title(record: &AgentRecord) -> String {
    let s = &record.summary;
    format!("{} \u{b7} {} \u{b7} {}", s.label, s.model, progress(s))
}

/// Shows agent `target` in place of the main agent, or the main agent again for `None`.
pub fn show_agent<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    monitor: Option<&AgentMonitor>,
    target: Option<u64>,
) -> Result<()> {
    match target.and_then(|id| monitor?.record(id)) {
        Some(record) => controller.enter_agent_view(record.summary.id, title(&record), frame(&record))?,
        None => controller.leave_agent_view()?,
    }
    Ok(())
}

/// The newest round of the same agent (same group and label) started after `record`, if any: the
/// view follows the agent into the rounds that continue its task.
fn newer_round(monitor: &AgentMonitor, record: &AgentRecord) -> Option<AgentRecord> {
    monitor
        .summaries()
        .into_iter()
        .filter(|s| s.id > record.summary.id && s.group == record.summary.group && s.label == record.summary.label)
        .max_by_key(|s| s.id)
        .and_then(|s| monitor.record(s.id))
}

/// Brings the shown agent up to date; returns whether the screen changed.
pub fn refresh<B: TerminalBackend>(controller: &mut TerminalController<B>, monitor: &AgentMonitor) -> Result<bool> {
    let Some(id) = controller.viewed_agent() else {
        return Ok(false);
    };
    let version = monitor.version();
    if controller.view_synced_version() == Some(version) {
        return Ok(false);
    }
    let changed = sync_viewed(controller, monitor, id)?;
    controller.set_view_synced_version(version);
    Ok(changed)
}

fn sync_viewed<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    monitor: &AgentMonitor,
    id: u64,
) -> Result<bool> {
    let Some(record) = monitor.record(id) else {
        controller.leave_agent_view()?;
        return Ok(true);
    };
    if let Some(newer) = newer_round(monitor, &record) {
        controller.enter_agent_view(newer.summary.id, title(&newer), frame(&newer))?;
        return Ok(true);
    }
    let titled = controller.set_view_title(title(&record));
    let synced = controller.sync_agent_view(frame(&record))?;
    Ok(titled || synced)
}

/// The text submitted with `key` while an agent is shown, taken from the prompt; `None` when the
/// key is not a submission to the shown agent.
pub fn take_submission<B: TerminalBackend>(controller: &mut TerminalController<B>, key: &KeyEvent) -> Option<String> {
    let state = controller.state();
    if controller.viewed_agent().is_none()
        || state.active_modal().is_some()
        || state.autocomplete.selected_item().is_some()
        || key.code != KeyCode::Enter
        || key.modifiers != KeyModifiers::NONE
    {
        return None;
    }
    let text = state.editor().text().trim().to_string();
    if text.is_empty() {
        return None;
    }
    controller.state_mut().editor_mut().set_text("");
    Some(text)
}

/// Where text typed while an agent is shown went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Delivered or refused; the status line says which.
    Handled,
    /// The agent's round has ended: the main agent is asked to continue its task with this prompt,
    /// so the round runs inside a normal, permission-checked turn.
    ContinueThroughMain(String),
}

fn continue_prompt(task_id: &str, label: &str, text: &str) -> String {
    format!(
        "Continue subagent task `{task_id}` ({label}) with the subagent tool, passing it my message below as the prompt, then report what it did.\n\n{text}"
    )
}

/// Sends `text` to the shown agent: straight to its running round, or through the main agent once
/// the round has ended.
pub fn route<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    engine: &crate::engine::AgentEngine,
    text: &str,
) -> Routed {
    if crate::repl::commands::is_slash_command(text) {
        controller.set_system_message("Commands act on the main session; press \u{2190} on an empty prompt to return");
        return Routed::Handled;
    }
    let record = controller
        .viewed_agent()
        .and_then(|id| engine.agent_monitor().record(id));
    let Some(summary) = record.map(|record| record.summary) else {
        return Routed::Handled;
    };
    let Some(task_id) = summary.task_id else {
        controller.set_system_message("This agent's round was not saved, so it cannot be messaged");
        return Routed::Handled;
    };
    if summary.status == AgentStatus::Running && engine.message_running_subagent(&task_id, text) {
        controller.set_system_message(format!(
            "Sent to {}; it reads it before its next model call",
            summary.label
        ));
        return Routed::Handled;
    }
    controller.set_system_message(format!("Asking the main agent to continue {}", summary.label));
    Routed::ContinueThroughMain(continue_prompt(&task_id, &summary.label, text))
}

#[cfg(test)]
#[path = "agent_focus_tests.rs"]
mod tests;
