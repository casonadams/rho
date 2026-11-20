use crate::presentation::ToolLine;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const MAX_EVENTS_PER_AGENT: usize = 500;
const MAX_EVENT_CHARS: usize = 4_000;
/// Assistant text is shown as rendered markdown, so it keeps more than tool output does.
const MAX_TEXT_CHARS: usize = 16_000;
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
    /// A tool call that has started and not finished yet.
    ToolCall {
        name: String,
        args: String,
    },
    /// A finished tool call; it replaces the matching pending `ToolCall`.
    Tool(ToolLine),
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

    /// The earliest started call that `line` finishes: same tool, same arguments.
    fn pending_call(&self, line: &ToolLine) -> Option<usize> {
        let args = line.arguments.to_string();
        self.events
            .iter()
            .position(|e| matches!(e, AgentEvent::ToolCall { name, args: a } if *name == line.name && *a == args))
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

    pub fn finish(&self, status: AgentStatus) {
        self.monitor.update(self.id, |agent| {
            agent.status = status;
            agent.finished_after = Some(agent.started.elapsed());
        });
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
    if text.chars().count() <= max_chars {
        return text;
    }
    let kept: String = text.chars().take(max_chars).collect();
    format!("{kept}\n[... truncated]")
}

fn clamp_event(event: AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::Task(text) => AgentEvent::Task(clamp_text(text, MAX_EVENT_CHARS)),
        AgentEvent::Text(text) => AgentEvent::Text(clamp_text(text, MAX_TEXT_CHARS)),
        AgentEvent::Thinking(text) => AgentEvent::Thinking(clamp_text(text, MAX_TEXT_CHARS)),
        AgentEvent::ToolCall { name, args } => AgentEvent::ToolCall {
            name,
            args: clamp_text(args, MAX_EVENT_CHARS),
        },
        AgentEvent::Tool(mut line) => {
            line.output = clamp_text(line.output, MAX_EVENT_CHARS);
            AgentEvent::Tool(line)
        }
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
        handle.event(AgentEvent::ToolCall {
            name: "rg".into(),
            args: "{}".into(),
        });
        handle.stream(Streamed::Text, "after");
        assert_eq!(monitor.record(handle.id()).unwrap().events.len(), 3);
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
        ToolLine {
            name: name.into(),
            arguments: args,
            is_error: false,
            output: output.into(),
            output_summary: output.into(),
            duration_ms: Some(5),
            file_change: None,
        }
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
            handle.event(AgentEvent::ToolCall {
                name: "read".into(),
                args: args.to_string(),
            });
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
