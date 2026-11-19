use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

const MAX_EVENTS_PER_AGENT: usize = 500;
const MAX_EVENT_CHARS: usize = 4_000;
const MAX_RETAINED_AGENTS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Done,
    /// Stopped before finishing (turn limit, failed model call, or no final answer) but returned its work so far.
    Incomplete,
    Failed,
    Cancelled,
}

impl AgentStatus {
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Running => "⟳",
            Self::Done => "✓",
            Self::Incomplete => "◐",
            Self::Failed => "✗",
            Self::Cancelled => "⊘",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    Task(String),
    Text(String),
    ToolCall {
        name: String,
        args: String,
    },
    ToolResult {
        name: String,
        output: String,
        is_error: bool,
    },
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
        }
    }

    fn push(&mut self, event: AgentEvent) {
        if self.events.len() == MAX_EVENTS_PER_AGENT {
            self.events.pop_front();
            self.dropped_events += 1;
        }
        self.events.push_back(clamp_event(event));
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

    pub fn turn(&self, turn: usize) {
        self.monitor.update(self.id, |agent| agent.turn = turn);
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

fn clamp_text(text: String) -> String {
    if text.chars().count() <= MAX_EVENT_CHARS {
        return text;
    }
    let kept: String = text.chars().take(MAX_EVENT_CHARS).collect();
    format!("{kept}\n[... truncated]")
}

fn clamp_event(event: AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::Task(text) => AgentEvent::Task(clamp_text(text)),
        AgentEvent::Text(text) => AgentEvent::Text(clamp_text(text)),
        AgentEvent::ToolCall { name, args } => AgentEvent::ToolCall {
            name,
            args: clamp_text(args),
        },
        AgentEvent::ToolResult { name, output, is_error } => AgentEvent::ToolResult {
            name,
            output: clamp_text(output),
            is_error,
        },
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
    fn truncates_long_event_text() {
        let monitor = AgentMonitor::default();
        let handle = monitor.start("g", "a", "m", 1);
        handle.event(AgentEvent::Text("x".repeat(MAX_EVENT_CHARS + 10)));
        let record = monitor.record(handle.id()).unwrap();
        let AgentEvent::Text(text) = &record.events[0] else {
            panic!("expected text event");
        };
        assert!(text.ends_with("[... truncated]"));
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
