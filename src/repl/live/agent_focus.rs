//! Shows a subagent in the terminal in place of the main agent, the way the main agent is shown:
//! its prompts, streamed reply, thinking, and tool cards, with the footer naming it. Text typed
//! while it is shown goes to it. `Left` on an empty prompt returns to the main agent.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rho_harness_core::agents::{AgentEvent, AgentMonitor, AgentRecord, AgentStatus};

use crate::error::Result;
use crate::repl::live::modal::agents::progress;
use crate::ui::interactive::{
    RunningTool, StreamingText, TerminalBackend, TerminalController, ToolStartRequest, TranscriptItem, ViewFrame,
};
use crate::ui::render::stream::StreamKind;

/// The agent's record as the main terminal shows it: settled items, the reply or thinking it is
/// still writing, and the tool it is running (as a live card, like the main agent's). Each item
/// carries the sequence number of the event it shows, which stays put as old events are dropped.
pub fn frame(record: AgentRecord) -> ViewFrame {
    let running = record.summary.status == AgentStatus::Running;
    let mut frame = ViewFrame::default();
    let mut events = record.events;
    if running && matches!(events.last(), Some(AgentEvent::Text(_) | AgentEvent::Thinking(_))) {
        frame.streaming = events.pop().and_then(streaming_text);
    }
    for (offset, event) in events.into_iter().enumerate() {
        let seq = record.dropped_events + offset;
        if let Some(item) = settled_item(event, running, &mut frame) {
            frame.items.push(item);
            frame.item_seqs.push(seq);
        }
    }
    frame
}

/// The transcript item `event` shows, or `None` when it shows none; a running agent's unfinished
/// call becomes the frame's live card instead.
fn settled_item(event: AgentEvent, running: bool, frame: &mut ViewFrame) -> Option<TranscriptItem> {
    match event {
        AgentEvent::Task(text) => Some(TranscriptItem::UserMessage(text.trim().to_string())),
        AgentEvent::Text(text) => Some(TranscriptItem::AssistantText(text)),
        AgentEvent::Thinking(text) => Some(TranscriptItem::Thinking(text)),
        AgentEvent::Tool(line) if RunningTool::has_transcript_card(&line.name) => Some(TranscriptItem::tool(line)),
        AgentEvent::Tool(_) => None,
        AgentEvent::ToolCall {
            name,
            args,
            output,
            dropped,
            file_change,
        } if running => {
            if let Some(card) = live_card(&name, args, file_change) {
                frame.running_tool = Some(card);
                frame.running_output = output;
                frame.running_output_start = dropped;
            }
            None
        }
        AgentEvent::ToolCall { name, .. } => Some(TranscriptItem::Notice(format!("\u{2717} {name} did not finish"))),
    }
}

fn streaming_text(event: AgentEvent) -> Option<StreamingText> {
    let (kind, text) = match event {
        AgentEvent::Text(text) => (StreamKind::Reply, text),
        AgentEvent::Thinking(text) => (StreamKind::Thinking, text),
        _ => return None,
    };
    Some(StreamingText { kind, text })
}

/// A running tool's card, for the tools the main agent shows one for.
fn live_card(
    name: &str,
    args: serde_json::Value,
    file_change: Option<rho_harness_core::presentation::FileChange>,
) -> Option<ToolStartRequest> {
    if !RunningTool::has_live_card(name) {
        return None;
    }
    Some(ToolStartRequest {
        name: name.to_string(),
        arguments: args,
        file_change,
    })
}

/// The footer title of the shown agent: who it is, its model, how far it is, and how many of its
/// earliest events were not kept.
fn title(record: &AgentRecord) -> String {
    let s = &record.summary;
    let title = format!("{} \u{b7} {} \u{b7} {}", s.label, s.model, progress(s));
    match record.dropped_events {
        0 => title,
        1 => format!("{title} \u{b7} 1 earlier event not kept"),
        n => format!("{title} \u{b7} {n} earlier events not kept"),
    }
}

fn enter<B: TerminalBackend>(controller: &mut TerminalController<B>, record: AgentRecord) -> Result<()> {
    let title = title(&record);
    controller.enter_agent_view(record.summary.id, title, frame(record))?;
    Ok(())
}

/// Shows agent `target` in place of the main agent, or the main agent again for `None`.
pub fn show_agent<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    monitor: Option<&AgentMonitor>,
    target: Option<u64>,
) -> Result<()> {
    match target.and_then(|id| monitor?.record(id)) {
        Some(record) => enter(controller, record)?,
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
        enter(controller, newer)?;
        return Ok(true);
    }
    let titled = controller.set_view_title(title(&record));
    let synced = controller.sync_agent_view(frame(record))?;
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
        controller.set_system_message("Not sent: the shown agent is no longer known; press \u{2190} to return");
        return Routed::Handled;
    };
    let Some(task_id) = summary.task_id else {
        controller.set_system_message("This agent's round was not saved, so it cannot be messaged");
        return Routed::Handled;
    };
    if summary.status == AgentStatus::Running {
        match engine.message_running_subagent(&task_id, text) {
            Ok(delivery) => {
                controller.set_system_message(match delivery {
                    rho_engine::mailbox::Delivery::Reply => {
                        format!("Answered {}'s question; it continues", summary.label)
                    }
                    rho_engine::mailbox::Delivery::Queued => {
                        format!("Sent to {}; it reads it before its next model call", summary.label)
                    }
                });
                return Routed::Handled;
            }
            Err(rho_engine::mailbox::SendError::NoMailbox) => {}
            Err(error) => {
                controller.set_system_message(format!("Not sent to {}: {error}", summary.label));
                return Routed::Handled;
            }
        }
    }
    controller.set_system_message(format!("Asking the main agent to continue {}", summary.label));
    Routed::ContinueThroughMain(continue_prompt(&task_id, &summary.label, text))
}

#[cfg(test)]
#[path = "agent_focus_tests.rs"]
mod tests;
