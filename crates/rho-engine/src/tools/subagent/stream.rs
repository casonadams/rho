//! Shows a round's turn-loop stream as it runs: its text and reasoning, tool calls, and progress
//! lines, and tallies its turns, tool calls, and tokens.

use super::runner::{Finish, SubagentRunner};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::runner::turn::runner::{AgentRunner, MultiTurnStreamItem, StreamEvent};
use crate::engine::runner::turn::streaming_tool::StreamingToolTracker;
use crate::engine::runner::turn::types::{CancellationSignal, unless_cancelled};
use futures::StreamExt;
use rho_harness_core::agents::{AgentEvent, AgentHandle};
use rho_harness_core::error::AppError;
use std::sync::Arc;

/// What a round's stream has shown so far: the tools of the model call in progress, and the tool
/// call whose arguments are streaming into a preview card.
#[derive(Default)]
struct Shown {
    pending_names: Vec<String>,
    streaming_tool: StreamingToolTracker,
}

/// What the round's turns added up to.
#[derive(Default)]
pub(super) struct Tally {
    pub(super) turns: usize,
    pub(super) tool_calls: usize,
    pub(super) tokens: u64,
    pub(super) cached_tokens: u64,
}

impl SubagentRunner {
    /// Reads the turn loop's stream to its end: shows its output, releases its display barriers, and
    /// adds its turns, tool calls, and tokens to `tally`. Stops early when `stop` fires.
    pub(super) async fn consume(
        &self,
        agent: AgentRunner,
        sink: &Arc<TerminalApprovalSink>,
        tracker: Option<&AgentHandle>,
        stop: &CancellationSignal,
        tally: &mut Tally,
    ) -> Finish {
        let mut stream = agent.stream();
        let mut shown = Shown::default();
        let mut output = None;
        loop {
            let Some(item) = unless_cancelled(Some(stop), stream.next()).await else {
                sink.flush_display();
                return Finish::Stopped;
            };
            match item {
                None => break,
                Some(Err(error)) => {
                    sink.flush_display();
                    return Finish::Failed(error);
                }
                Some(Ok(MultiTurnStreamItem::FinalResponse(resp))) => output = Some(resp.output),
                Some(Ok(item)) => self.show(item, sink, tracker, tally, &mut shown),
            }
        }
        sink.flush_display();
        match output {
            Some(text) => Finish::Done(text),
            None if stop.is_cancelled() => Finish::Stopped,
            None => Finish::Failed(AppError::Other(anyhow::anyhow!("the turn ended unexpectedly"))),
        }
    }

    fn show(
        &self,
        item: MultiTurnStreamItem,
        sink: &Arc<TerminalApprovalSink>,
        tracker: Option<&AgentHandle>,
        tally: &mut Tally,
        shown: &mut Shown,
    ) {
        shown.streaming_tool.observe_boundary(&item, sink);
        match item {
            MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Text(text)) => sink.emit_text(&text),
            MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(text)) => sink.emit_reasoning(&text),
            MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Arguments { id, name, json }) => {
                sink.flush_reasoning();
                shown.streaming_tool.handle_delta(&id, &name, &json, sink);
            }
            MultiTurnStreamItem::ToolCall { tool_call } => {
                tally.tool_calls += 1;
                shown.pending_names.push(tool_call.function.name);
            }
            MultiTurnStreamItem::ModelTurnRetried(retry) => {
                tally.tool_calls = tally.tool_calls.saturating_sub(retry.discarded_tool_calls);
                shown.pending_names.clear();
                sink.flush_display();
            }
            MultiTurnStreamItem::CompletionCall(call) => {
                tally.turns += 1;
                let cached = call.usage.cached_input_tokens.unwrap_or(0);
                let written = call.usage.cache_creation_input_tokens.unwrap_or(0);
                tally.tokens += call.usage.input_tokens + cached + written + call.usage.output_tokens;
                tally.cached_tokens += cached;
                sink.flush_display();
                if let Some(tracker) = tracker {
                    tracker.turn(tally.turns);
                }
                if !shown.pending_names.is_empty() {
                    self.report_progress(tally.turns, &shown.pending_names);
                    shown.pending_names.clear();
                }
            }
            MultiTurnStreamItem::DisplayBarrier(drained) => {
                sink.flush_display();
                drained.notify_one();
            }
            MultiTurnStreamItem::MessagesDelivered(messages) => {
                if let Some(tracker) = tracker {
                    for message in messages {
                        tracker.event(AgentEvent::Task(message));
                    }
                }
            }
            MultiTurnStreamItem::ToolExecutionCommitted | MultiTurnStreamItem::FinalResponse(_) => {}
        }
    }

    fn report_progress(&self, turn: usize, names: &[String]) {
        self.ctx.progress.stream_chunk(&format!(
            "{} turn {turn}/{}: {}\n",
            self.label(),
            self.max_turns,
            names.join(", ")
        ));
    }
}
