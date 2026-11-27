use crate::presentation::{FileChange, ToolLine};
use crate::text::truncate_chars;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const MAX_EVENTS_PER_AGENT: usize = 500;
const MAX_EVENT_CHARS: usize = 4_000;
/// Assistant text is shown as rendered markdown, so it keeps more than tool output does.
const MAX_TEXT_CHARS: usize = 16_000;
/// What a running tool's card keeps of the output streamed to it; the oldest output goes first.
const MAX_CALL_OUTPUT_BYTES: usize = 200_000;
const MAX_RETAINED_AGENTS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Done,
    /// Paused for the parent: it asked a question, or stopped early and summarized its work so far.
    InputRequired,
    Failed,
    Cancelled,
}

impl AgentStatus {
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Running => "⟳",
            Self::Done => "✓",
            Self::InputRequired => "◐",
            Self::Failed => "✗",
            Self::Cancelled => "⊘",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    Task(String),
    Text(String),
    Thinking(String),
    /// A tool call that has started and not finished yet, with the output streamed to its card so far.
    ToolCall {
        name: String,
        args: serde_json::Value,
        output: String,
        /// Bytes of streamed output cut from the front of `output` to keep it bounded.
        dropped: usize,
        /// The state of the file the call changes, taken before it runs.
        file_change: Option<FileChange>,
    },
    /// A finished tool call; it replaces the matching pending `ToolCall`.
    Tool(ToolLine),
}

impl AgentEvent {
    /// A tool call that has just started, with nothing streamed to its card yet.
    pub fn tool_call(name: impl Into<String>, args: serde_json::Value, file_change: Option<FileChange>) -> Self {
        Self::ToolCall {
            name: name.into(),
            args,
            output: String::new(),
            dropped: 0,
            file_change,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSummary {
    pub id: u64,
    pub group: String,
    pub label: String,
    pub model: String,
    pub status: AgentStatus,
    pub turn: usize,
    pub max_turns: usize,
    pub elapsed: Duration,
    /// The subagent task this round belongs to, when it is saved and can be continued.
    pub task_id: Option<String>,
    /// A permission prompt for one of this agent's tool calls is waiting for the user.
    pub awaiting_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecord {
    pub summary: AgentSummary,
    pub events: Vec<AgentEvent>,
    pub dropped_events: usize,
}

#[derive(Debug)]
struct TrackedAgent {
    id: u64,
    group: String,
    label: String,
    model: String,
    status: AgentStatus,
    turn: usize,
    max_turns: usize,
    started: Instant,
    finished_after: Option<Duration>,
    events: VecDeque<AgentEvent>,
    dropped_events: usize,
    task_id: Option<String>,
    awaiting_approval: bool,
    /// The kind of event still growing from streamed deltas, until any other event arrives.
    streaming: Option<Streamed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streamed {
    Text,
    Thinking,
}

impl Streamed {
    fn event(self, text: String) -> AgentEvent {
        match self {
            Self::Text => AgentEvent::Text(text),
            Self::Thinking => AgentEvent::Thinking(text),
        }
    }
}

impl TrackedAgent {
    fn summary(&self) -> AgentSummary {
        AgentSummary {
            id: self.id,
            group: self.group.clone(),
            label: self.label.clone(),
            model: self.model.clone(),
            status: self.status,
            turn: self.turn,
            max_turns: self.max_turns,
            elapsed: self.finished_after.unwrap_or_else(|| self.started.elapsed()),
            task_id: self.task_id.clone(),
            awaiting_approval: self.awaiting_approval,
        }
    }

    fn push(&mut self, event: AgentEvent) {
        self.streaming = None;
        let event = clamp_event(event);
        if let AgentEvent::Tool(line) = &event
            && let Some(pending) = self.pending_call(line)
        {
            self.events[pending] = event;
            return;
        }
        if self.events.len() == MAX_EVENTS_PER_AGENT {
            self.events.pop_front();
            self.dropped_events += 1;
        }
        self.events.push_back(event);
    }

    /// Grows the streamed text or thinking at the end of the events, starting it if none is open.
    fn stream(&mut self, kind: Streamed, delta: &str) {
        if self.streaming == Some(kind)
            && let Some(last) = self.events.back_mut()
            && let AgentEvent::Text(text) | AgentEvent::Thinking(text) = last
        {
            if text.chars().count() < MAX_TEXT_CHARS {
                text.push_str(delta);
            }
            return;
        }
        self.push(kind.event(delta.to_string()));
        self.streaming = Some(kind);
    }

    /// Ends any streamed event, replacing streamed text with `final_text` (the model's whole reply).
    fn finish_text(&mut self, final_text: &str) {
        let streamed_text = self.streaming == Some(Streamed::Text);
        self.streaming = None;
        let final_text = final_text.trim();
        if final_text.is_empty() {
            return;
        }
        if streamed_text && let Some(AgentEvent::Text(text)) = self.events.back_mut() {
            *text = clamp_text(final_text.to_string(), MAX_TEXT_CHARS);
            return;
        }
        self.push(AgentEvent::Text(final_text.to_string()));
    }

    /// The earliest started call that `line` finishes: same tool and arguments, else the earliest of
    /// the same tool, since a previewed call is started before its arguments are complete.
    fn pending_call(&self, line: &ToolLine) -> Option<usize> {
        let pending = |exact: bool| {
            self.events.iter().position(|e| {
                matches!(e, AgentEvent::ToolCall { name, args, .. }
                    if *name == line.name && (!exact || *args == line.arguments))
            })
        };
        pending(true).or_else(|| pending(false))
    }

    /// Starts or updates the call `name` whose arguments are still streaming: it replaces an
    /// unfinished call just before it, the earlier state of this preview or the preview of a call
    /// streamed before it in the same reply, which is shown again when that call starts.
    fn preview_call(&mut self, name: &str, args: serde_json::Value, file_change: Option<FileChange>) {
        if matches!(self.events.back(), Some(AgentEvent::ToolCall { .. })) {
            self.events.pop_back();
        }
        self.push(AgentEvent::tool_call(name, args, file_change));
    }

    /// Drops the latest unfinished call, which will not produce a tool line.
    fn end_call(&mut self) {
        if let Some(at) = self
            .events
            .iter()
            .rposition(|e| matches!(e, AgentEvent::ToolCall { .. }))
        {
            self.events.remove(at);
        }
    }

    /// Adds output streamed to the card of the latest unfinished call.
    fn call_output(&mut self, chunk: &str) {
        let Some(AgentEvent::ToolCall { output, dropped, .. }) = self.events.back_mut() else {
            return;
        };
        output.push_str(chunk);
        if output.len() > 2 * MAX_CALL_OUTPUT_BYTES {
            let cut = output.ceil_char_boundary(output.len() - MAX_CALL_OUTPUT_BYTES);
            output.drain(..cut);
            *dropped += cut;
        }
    }
}

#[derive(Debug, Default)]
struct MonitorState {
    version: u64,
    next_id: u64,
    agents: Vec<TrackedAgent>,
}

#[derive(Clone, Default)]
pub struct AgentMonitor {
    state: Arc<Mutex<MonitorState>>,
}

impl std::fmt::Debug for AgentMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentMonitor")
            .field("version", &self.version())
            .finish()
    }
}

impl AgentMonitor {
    fn lock(&self) -> MutexGuard<'_, MonitorState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[must_use]
    pub fn start(&self, group: &str, label: &str, model: &str, max_turns: usize) -> AgentHandle {
        let mut state = self.lock();
        state.next_id += 1;
        let id = state.next_id;
        state.agents.push(TrackedAgent {
            id,
            group: group.to_string(),
            label: label.to_string(),
            model: model.to_string(),
            status: AgentStatus::Running,
            turn: 0,
            max_turns,
            started: Instant::now(),
            finished_after: None,
            events: VecDeque::new(),
            dropped_events: 0,
            task_id: None,
            awaiting_approval: false,
            streaming: None,
        });
        evict_finished(&mut state.agents);
        state.version += 1;
        AgentHandle {
            monitor: self.clone(),
            id,
        }
    }

    #[must_use]
    pub fn version(&self) -> u64 {
        self.lock().version
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().agents.is_empty()
    }

    #[must_use]
    pub fn summaries(&self) -> Vec<AgentSummary> {
        self.lock().agents.iter().map(TrackedAgent::summary).collect()
    }

    #[must_use]
    pub fn record(&self, id: u64) -> Option<AgentRecord> {
        let state = self.lock();
        let agent = state.agents.iter().find(|a| a.id == id)?;
        Some(AgentRecord {
            summary: agent.summary(),
            events: agent.events.iter().cloned().collect(),
            dropped_events: agent.dropped_events,
        })
    }

    fn update(&self, id: u64, apply: impl FnOnce(&mut TrackedAgent)) {
        let mut state = self.lock();
        if let Some(agent) = state.agents.iter_mut().find(|a| a.id == id) {
            apply(agent);
            state.version += 1;
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentHandle {
    monitor: AgentMonitor,
    id: u64,
}

impl AgentHandle {
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn event(&self, event: AgentEvent) {
        self.monitor.update(self.id, |agent| agent.push(event));
    }

    /// Starts or updates the call `name` whose arguments are still streaming, replacing its earlier state.
    pub fn preview_call(&self, name: &str, args: serde_json::Value, file_change: Option<FileChange>) {
        self.monitor
            .update(self.id, |agent| agent.preview_call(name, args, file_change));
    }

    /// Adds output streamed to the card of the latest unfinished call.
    pub fn call_output(&self, chunk: &str) {
        self.monitor.update(self.id, |agent| agent.call_output(chunk));
    }

    /// Drops the latest unfinished call, which will not produce a tool line, such as a preview of
    /// a call the model retried.
    pub fn end_call(&self) {
        self.monitor.update(self.id, TrackedAgent::end_call);
    }

    /// Adds a streamed piece of the model's reply, or of its reasoning, to the view.
    pub fn stream(&self, kind: Streamed, delta: &str) {
        self.monitor.update(self.id, |agent| agent.stream(kind, delta));
    }

    /// Ends the streamed reply and sets its final text.
    pub fn finish_text(&self, final_text: &str) {
        self.monitor.update(self.id, |agent| agent.finish_text(final_text));
    }

    pub fn turn(&self, turn: usize) {
        self.monitor.update(self.id, |agent| agent.turn = turn);
    }

    /// Links the round to its saved subagent task, so the view can continue it.
    pub fn set_task(&self, task_id: &str) {
        let task_id = task_id.to_string();
        self.monitor.update(self.id, |agent| agent.task_id = Some(task_id));
    }

    /// Marks the agent as waiting for the user to answer a permission prompt, until the guard drops.
    #[must_use]
    pub fn awaiting_approval(&self) -> AwaitingApproval {
        self.monitor.update(self.id, |agent| agent.awaiting_approval = true);
        AwaitingApproval(self.clone())
    }

    pub fn finish(&self, status: AgentStatus) {
        self.monitor.update(self.id, |agent| {
            agent.status = status;
            agent.finished_after = Some(agent.started.elapsed());
        });
    }
}

/// Clears the agent's waiting-for-approval mark when the prompt is answered or abandoned.
pub struct AwaitingApproval(AgentHandle);

impl Drop for AwaitingApproval {
    fn drop(&mut self) {
        self.0
            .monitor
            .update(self.0.id, |agent| agent.awaiting_approval = false);
    }
}

fn evict_finished(agents: &mut Vec<TrackedAgent>) {
    while agents.len() > MAX_RETAINED_AGENTS {
        let Some(oldest) = agents.iter().position(|a| a.status != AgentStatus::Running) else {
            return;
        };
        agents.remove(oldest);
    }
}

fn clamp_text(text: String, max_chars: usize) -> String {
    match truncate_chars(&text, max_chars) {
        (kept, true) => format!("{kept}\n[... truncated]"),
        (_, false) => text,
    }
}

fn clamp_event(event: AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::Task(text) => AgentEvent::Task(clamp_text(text, MAX_EVENT_CHARS)),
        AgentEvent::Text(text) => AgentEvent::Text(clamp_text(text, MAX_TEXT_CHARS)),
        AgentEvent::Thinking(text) => AgentEvent::Thinking(clamp_text(text, MAX_TEXT_CHARS)),
        AgentEvent::Tool(mut line) => {
            line.output = clamp_text(line.output, MAX_EVENT_CHARS);
            AgentEvent::Tool(line)
        }
        call @ AgentEvent::ToolCall { .. } => call,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_lifecycle_and_bumps_version_on_every_change() {
        let monitor = AgentMonitor::default();
        assert!(monitor.is_empty());
        let handle = monitor.start("refine › round 1", "scan #1", "claude/claude-haiku-5-5", 10);
        let after_start = monitor.version();
        handle.turn(3);
        handle.event(AgentEvent::Text("looking".into()));
        handle.finish(AgentStatus::Done);
        assert!(monitor.version() > after_start);

        let record = monitor.record(handle.id()).unwrap();
        assert_eq!(record.summary.status, AgentStatus::Done);
        assert_eq!(record.summary.turn, 3);
        assert_eq!(record.summary.group, "refine › round 1");
        assert_eq!(record.summary.model, "claude/claude-haiku-5-5");
        assert_eq!(record.events, vec![AgentEvent::Text("looking".into())]);
    }

    #[test]
    fn caps_events_and_counts_dropped_ones() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        for i in 0..MAX_EVENTS_PER_AGENT + 3 {
            handle.event(AgentEvent::Text(i.to_string()));
        }
        let record = monitor.record(handle.id()).unwrap();
        assert_eq!(record.events.len(), MAX_EVENTS_PER_AGENT);
        assert_eq!(record.dropped_events, 3);
        assert_eq!(record.events[0], AgentEvent::Text("3".into()));
    }

    #[test]
    fn streamed_text_and_thinking_grow_in_place_and_the_final_reply_replaces_the_text() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.stream(Streamed::Thinking, "hmm ");
        handle.stream(Streamed::Thinking, "ok");
        handle.stream(Streamed::Text, "Hel");
        handle.stream(Streamed::Text, "lo");
        assert_eq!(
            monitor.record(handle.id()).unwrap().events,
            [AgentEvent::Thinking("hmm ok".into()), AgentEvent::Text("Hello".into())]
        );

        handle.finish_text("  Hello there  ");
        handle.stream(Streamed::Text, "next");
        let events = monitor.record(handle.id()).unwrap().events;
        assert_eq!(
            events,
            [
                AgentEvent::Thinking("hmm ok".into()),
                AgentEvent::Text("Hello there".into()),
                AgentEvent::Text("next".into()),
            ],
            "a new delta after the reply ended starts a new block"
        );
    }

    #[test]
    fn a_reply_that_never_streamed_is_added_whole_and_an_empty_one_adds_nothing() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.finish_text("   ");
        handle.finish_text(" whole reply ");
        assert_eq!(
            monitor.record(handle.id()).unwrap().events,
            [AgentEvent::Text("whole reply".into())]
        );
    }

    #[test]
    fn a_tool_call_ends_the_streamed_block() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.stream(Streamed::Text, "before");
        handle.event(AgentEvent::tool_call("rg", serde_json::json!({}), None));
        handle.stream(Streamed::Text, "after");
        assert_eq!(monitor.record(handle.id()).unwrap().events.len(), 3);
    }

    #[test]
    fn an_agent_is_marked_waiting_for_approval_while_its_prompt_is_open() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        let waiting = handle.awaiting_approval();
        assert!(monitor.record(handle.id()).unwrap().summary.awaiting_approval);
        drop(waiting);
        assert!(!monitor.record(handle.id()).unwrap().summary.awaiting_approval);
    }

    #[test]
    fn truncates_long_event_text() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.event(AgentEvent::Task("x".repeat(MAX_EVENT_CHARS + 10)));
        handle.event(AgentEvent::Text("y".repeat(MAX_TEXT_CHARS + 10)));
        let record = monitor.record(handle.id()).unwrap();
        let [AgentEvent::Task(task), AgentEvent::Text(text)] = &record.events[..] else {
            panic!("expected task and text events");
        };
        assert!(task.ends_with("[... truncated]") && task.chars().count() < MAX_EVENT_CHARS + 20);
        assert!(text.ends_with("[... truncated]") && text.chars().count() > MAX_EVENT_CHARS);
    }

    fn line(name: &str, args: serde_json::Value, output: &str) -> ToolLine {
        ToolLine::new(name, args)
            .with_output(output)
            .with_output_summary(output)
            .with_duration_ms(5)
    }

    #[test]
    fn a_large_tool_call_keeps_its_arguments_and_is_replaced_by_its_finished_line() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        let args = serde_json::json!({ "path": "a.rs", "text": "x".repeat(MAX_EVENT_CHARS * 3) });
        handle.event(AgentEvent::tool_call("write", args.clone(), None));
        let pending = monitor.record(handle.id()).unwrap().events;
        assert_eq!(pending, [AgentEvent::tool_call("write", args.clone(), None)]);
        handle.event(AgentEvent::Tool(line("write", args.clone(), "ok")));
        assert_eq!(
            monitor.record(handle.id()).unwrap().events,
            [AgentEvent::Tool(line("write", args, "ok"))]
        );
    }

    #[test]
    fn a_previewed_call_is_updated_in_place_streams_output_and_is_finished_by_its_line() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.preview_call("write", serde_json::json!({}), None);
        handle.call_output("one\n");
        handle.preview_call("write", serde_json::json!({ "path": "a.rs" }), None);
        handle.call_output("one\ntwo\n");
        let [AgentEvent::ToolCall { args, output, .. }] = &monitor.record(handle.id()).unwrap().events[..] else {
            panic!("the preview is one call");
        };
        assert_eq!(
            (args, output.as_str()),
            (&serde_json::json!({ "path": "a.rs" }), "one\ntwo\n")
        );

        let full = serde_json::json!({ "path": "a.rs", "text": "one\ntwo\n" });
        handle.event(AgentEvent::Tool(line("write", full.clone(), "ok")));
        assert_eq!(
            monitor.record(handle.id()).unwrap().events,
            [AgentEvent::Tool(line("write", full, "ok"))]
        );
    }

    #[test]
    fn calls_previewed_one_after_another_leave_no_stale_card_once_they_run() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        let write = serde_json::json!({ "path": "a.rs", "text": "x" });
        let edit = serde_json::json!({ "path": "b.rs", "edits": [] });
        handle.preview_call("write", serde_json::json!({ "path": "a.rs" }), None);
        handle.preview_call("edit", serde_json::json!({ "path": "b.rs" }), None);

        handle.preview_call("write", write.clone(), None);
        handle.event(AgentEvent::Tool(line("write", write.clone(), "ok")));
        handle.event(AgentEvent::tool_call("edit", edit.clone(), None));
        handle.event(AgentEvent::Tool(line("edit", edit.clone(), "ok")));

        assert_eq!(
            monitor.record(handle.id()).unwrap().events,
            [
                AgentEvent::Tool(line("write", write, "ok")),
                AgentEvent::Tool(line("edit", edit, "ok")),
            ]
        );
    }

    #[test]
    fn a_calls_streamed_output_keeps_the_latest_and_counts_what_it_cut() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.preview_call("bash", serde_json::json!({}), None);
        let piece = "héllo woréd\n".repeat(1000);
        let pieces = 2 * MAX_CALL_OUTPUT_BYTES / piece.len() + 3;
        for _ in 0..pieces {
            handle.call_output(&piece);
        }
        let events = monitor.record(handle.id()).unwrap().events;
        let [AgentEvent::ToolCall { output, dropped, .. }] = &events[..] else {
            panic!("one running call");
        };
        assert!(*dropped > 0 && output.len() <= 2 * MAX_CALL_OUTPUT_BYTES);
        assert_eq!(dropped + output.len(), pieces * piece.len());
        assert!(output.ends_with(&piece));
    }

    #[test]
    fn a_finished_tool_replaces_its_pending_call_in_place() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        let (a, b) = (
            serde_json::json!({ "path": "a.rs" }),
            serde_json::json!({ "path": "b.rs" }),
        );
        for args in [&a, &b] {
            handle.event(AgentEvent::tool_call("read", args.clone(), None));
        }
        handle.event(AgentEvent::Tool(line("read", b.clone(), "B")));
        handle.event(AgentEvent::Text("after".into()));
        handle.event(AgentEvent::Tool(line("read", a.clone(), "A")));
        handle.set_task("scan-1234");

        let record = monitor.record(handle.id()).unwrap();
        assert_eq!(
            record.events,
            [
                AgentEvent::Tool(line("read", a, "A")),
                AgentEvent::Tool(line("read", b, "B")),
                AgentEvent::Text("after".into()),
            ]
        );
        assert_eq!(record.summary.task_id.as_deref(), Some("scan-1234"));
    }

    #[test]
    fn evicts_oldest_finished_agents_but_keeps_running_ones() {
        let monitor = AgentMonitor::default();
        let first = monitor.start("g", "running", "m", 1);
        for _ in 0..MAX_RETAINED_AGENTS {
            monitor.start("g", "done", "m", 1).finish(AgentStatus::Done);
        }
        let summaries = monitor.summaries();
        assert_eq!(summaries.len(), MAX_RETAINED_AGENTS);
        assert!(summaries.iter().any(|s| s.id == first.id()));
    }
}
