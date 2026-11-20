use super::evidence::{OutcomeStatus, ToolOutcome, evidence, problem_count};
use super::recovery::{attempt_recovery_synthesis, partial_result};
use super::snapshot::Snapshot;
use super::workspace::{WorkspaceChange, bash_file_arguments, normalize};
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::StructuralUsage;
use crate::engine::runner::history::without_unanswered_tool_calls;
use crate::engine::runner::turn::runner::{DispatchDecision, apply_tool_outcome_hooks, evaluate_tool_dispatch};
use crate::provider::adapter::{ModelCompletionRequest, ModelToolDefinition};
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use rho_harness_core::agents::{AgentEvent, AgentHandle, AgentStatus};
use rho_harness_core::model::{AssistantContent, ChatMessage, TextContent, ToolCall, UserContent};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const MAX_TOOL_OUTPUT_CHARS: usize = 20_000;
const MAX_PARENT_MESSAGES: usize = 20;
const MAX_PARENT_MESSAGE_CHARS: usize = 1_500;
pub(crate) const RUN_FOOTER_PREFIX: &str = "\n\n[subagent";
const DEFAULT_MONITOR_GROUP: &str = "subagents";
pub(crate) const READ_ONLY_TOOLS: [&str; 5] = ["read", "fd", "rg", "web_fetch", "web_search"];
const FILE_CHANGING_TOOLS: [&str; 2] = ["write", "edit"];
const FILE_READING_TOOLS: [&str; 3] = ["read", "write", "edit"];
pub(crate) const ASK_PARENT_TOOL: &str = "ask_parent";
const ASK_PARENT_DESCRIPTION: &str = "Ask the agent that delegated this task a question and pause until it replies. Use it only when you cannot proceed without a decision or information you cannot find yourself; the reply arrives as this tool's result.";
const ASK_PARENT_GUIDANCE: &str = "If you cannot proceed without a decision or information only the delegating agent has, call ask_parent with a specific question instead of guessing; your work so far is kept while you wait.\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundEnd {
    Answered(String),
    /// The subagent called `ask_parent` and is waiting for a reply.
    Asked(String),
    /// Stopped before a final answer (turn limit or empty reply) and summarized its work so far.
    Stopped {
        reason: String,
        summary: String,
    },
    /// A model call failed after the subagent gathered findings, which the text reports.
    Partial(String),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Round {
    pub end: RoundEnd,
    pub transcript: Vec<ChatMessage>,
    pub turns: usize,
    pub tool_calls: usize,
    pub tokens: u64,
    pub changed_files: BTreeSet<String>,
    /// Every uncommitted-file change in the git work tree during the round, whoever made it.
    pub workspace_changes: BTreeSet<WorkspaceChange>,
    /// False when the workspace comparison could not cover every file (a non-git walk hit its limit).
    pub workspace_complete: bool,
    pub tool_log: Vec<ToolOutcome>,
    /// Files the subagent read, wrote, or edited, relative to the workspace root.
    pub touched_files: BTreeSet<String>,
}

impl Round {
    #[must_use]
    pub fn problem_count(&self) -> usize {
        problem_count(&self.tool_log)
    }

    /// Ended early with work the parent can build on or continue.
    #[must_use]
    pub fn is_incomplete(&self) -> bool {
        matches!(
            self.end,
            RoundEnd::Asked(_) | RoundEnd::Stopped { .. } | RoundEnd::Partial(_)
        )
    }

    /// The text the parent receives above the footer, or the error when the round produced nothing usable.
    pub fn body(&self) -> Result<String, String> {
        match &self.end {
            RoundEnd::Answered(text) => Ok(text.trim().to_string()),
            RoundEnd::Asked(question) => Ok(format!("Question for you: {question}")),
            RoundEnd::Stopped { reason, summary } => Ok(format!(
                "Incomplete: subagent {reason} before finishing; this summary covers its work so far.\n\n{summary}"
            )),
            RoundEnd::Partial(text) => Ok(text.clone()),
            RoundEnd::Failed(error) => Err(error.clone()),
            RoundEnd::Cancelled => Err("Subagent interrupted by operator".to_string()),
        }
    }
}

fn ask_parent_definition() -> ModelToolDefinition {
    ModelToolDefinition {
        name: ASK_PARENT_TOOL.to_string(),
        description: ASK_PARENT_DESCRIPTION.to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "question": { "type": "string", "description": "The specific question to ask" } },
            "required": ["question"]
        }),
    }
}

fn asked_calls(message: &ChatMessage) -> Vec<&ToolCall> {
    let ChatMessage::Assistant { content, .. } = message else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|c| match c {
            AssistantContent::ToolCall(call) if call.function.name == ASK_PARENT_TOOL => Some(call),
            _ => None,
        })
        .collect()
}

/// Answers the `ask_parent` calls still open in the last assistant message; returns whether any were open.
pub(crate) fn answer_pending_questions(transcript: &mut Vec<ChatMessage>, answer: &str) -> bool {
    let Some(asked_at) = transcript
        .iter()
        .rposition(|m| matches!(m, ChatMessage::Assistant { .. }))
    else {
        return false;
    };
    let answered: BTreeSet<&str> = transcript[asked_at + 1..]
        .iter()
        .filter_map(|m| match m {
            ChatMessage::User { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|c| match c {
            UserContent::ToolResult(result) => Some(result.call.as_str()),
            _ => None,
        })
        .collect();
    let open: Vec<ToolCall> = asked_calls(&transcript[asked_at])
        .into_iter()
        .filter(|call| !answered.contains(call.id.as_str()))
        .cloned()
        .collect();
    for call in &open {
        transcript.push(tool_result_message(call, answer.to_string()));
    }
    !open.is_empty()
}

/// What a round starts from: a new instruction, or a reply to the subagent's open `ask_parent` question.
#[derive(Debug, Clone, Copy)]
pub enum RoundInput<'a> {
    Prompt(&'a str),
    Reply(&'a str),
}

const NO_REPLY: &str = "No reply; the delegating agent sent new instructions instead.";

pub fn build_subagent_prompt(prompt: &str, context_slice: Option<&str>) -> String {
    match context_slice {
        Some(slice) if !slice.trim().is_empty() => {
            format!("Context:\n{}\n\nTask:\n{}", slice.trim(), prompt.trim())
        }
        _ => prompt.trim().to_string(),
    }
}

pub fn filter_tools_for_role(tools: &[Arc<dyn EngineTool>], role: SubagentRole) -> Vec<Arc<dyn EngineTool>> {
    let allowed = role.allowed_tools();
    let mut filtered: Vec<Arc<dyn EngineTool>> =
        tools.iter().filter(|t| allowed.contains(&t.name())).cloned().collect();
    filtered.sort_by(|a, b| a.name().cmp(b.name()));
    filtered
}

#[derive(Clone)]
pub struct SubagentRunner {
    model: ModelHandle,
    instructions: String,
    tools: Vec<Arc<dyn EngineTool>>,
    max_turns: usize,
    ctx: TurnToolContext,
    label: String,
    monitor_group: String,
    monitor_label: String,
    position: Option<String>,
    parent_channel: bool,
    /// Runs in its own worktree, where nothing else writes.
    isolated: bool,
    inbox: Option<super::background::Inbox>,
}

struct RunState {
    task: String,
    task_pending: bool,
    messages: Vec<ChatMessage>,
    usage: StructuralUsage,
    turns: usize,
    tool_calls: usize,
    tracker: Option<AgentHandle>,
    changed_files: BTreeSet<String>,
    touched_files: BTreeSet<String>,
    tool_log: Vec<ToolOutcome>,
    /// Path -> the bash command that was running when the path changed.
    bash_changes: BTreeMap<String, String>,
    root: std::path::PathBuf,
    question: Option<String>,
}

struct CallOutcome {
    message: ChatMessage,
    stop: Option<String>,
    is_error: bool,
    output: String,
    bash_changes: Vec<String>,
    bash_reads: Vec<String>,
}

fn tool_result_text(message: &ChatMessage) -> String {
    let ChatMessage::User { content } = message else {
        return String::new();
    };
    content
        .iter()
        .filter_map(|c| match c {
            UserContent::ToolResult(result) => Some(result),
            _ => None,
        })
        .flat_map(|result| result.content.iter().filter_map(|part| part.as_text()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Changes found by the per-command snapshots around the agent's bash calls.
fn bash_changes(state: &RunState) -> BTreeSet<WorkspaceChange> {
    state
        .bash_changes
        .iter()
        .map(|(path, command)| WorkspaceChange {
            path: path.clone(),
            deleted: !state.root.join(path).exists(),
            by: Some(command.clone()),
        })
        .collect()
}

fn tool_result_message(call: &ToolCall, output: String) -> ChatMessage {
    ChatMessage::User {
        content: vec![UserContent::ToolResult(rho_harness_core::model::ToolResult::new(
            call.id.clone(),
            call.function.name.clone(),
            output,
        ))],
    }
}

fn assistant_tool_message(content: &str, calls: &[ToolCall]) -> ChatMessage {
    let mut parts = Vec::new();
    if !content.is_empty() {
        parts.push(AssistantContent::Text(TextContent::new(content)));
    }
    parts.extend(calls.iter().cloned().map(AssistantContent::ToolCall));
    ChatMessage::Assistant {
        id: None,
        content: parts,
    }
}

fn truncate_output(output: String) -> String {
    let total = output.chars().count();
    if total <= MAX_TOOL_OUTPUT_CHARS {
        return output;
    }
    let kept: String = output.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
    format!("{kept}\n[truncated {} characters]", total - MAX_TOOL_OUTPUT_CHARS)
}

fn message_text(message: &ChatMessage) -> Option<(&'static str, String)> {
    let (speaker, text) = match message {
        ChatMessage::User { content } => (
            "User",
            content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ChatMessage::Assistant { content, .. } => (
            "Assistant",
            content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ChatMessage::System { .. } => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| (speaker, truncate_middle(text)))
}

fn truncate_middle(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_PARENT_MESSAGE_CHARS {
        return text.to_string();
    }
    let head_len = MAX_PARENT_MESSAGE_CHARS / 2;
    let tail_len = MAX_PARENT_MESSAGE_CHARS - head_len;
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(total - tail_len).collect();
    format!(
        "{head}\n[... {} characters omitted ...]\n{tail}",
        total - MAX_PARENT_MESSAGE_CHARS
    )
}

/// Text-only transcript of the most recent parent messages; tool calls, tool output and reasoning are left out.
pub(crate) fn render_parent_conversation(history: &[ChatMessage]) -> String {
    let lines: Vec<String> = history
        .iter()
        .filter_map(message_text)
        .map(|(speaker, text)| format!("{speaker}: {text}"))
        .collect();
    let recent = &lines[lines.len().saturating_sub(MAX_PARENT_MESSAGES)..];
    if recent.is_empty() {
        return String::new();
    }
    format!("Parent conversation (most recent last):\n{}", recent.join("\n\n"))
}

fn track_response(tracker: Option<&AgentHandle>, turn: usize, content: &str, calls: &[ToolCall]) {
    let Some(tracker) = tracker else {
        return;
    };
    tracker.turn(turn);
    if !content.trim().is_empty() {
        tracker.event(AgentEvent::Text(content.trim().to_string()));
    }
    for call in calls {
        tracker.event(AgentEvent::ToolCall {
            name: call.function.name.clone(),
            args: call.function.arguments.to_string(),
        });
    }
}

fn interrupted() -> RoundEnd {
    RoundEnd::Cancelled
}

impl SubagentRunner {
    #[must_use]
    pub fn new(
        model: ModelHandle,
        role: SubagentRole,
        available_tools: &[Arc<dyn EngineTool>],
        max_turns: usize,
    ) -> Self {
        let mut runner = Self::new_custom(
            model,
            role.system_instructions().to_string(),
            filter_tools_for_role(available_tools, role),
            max_turns,
        );
        runner.monitor_label = role.as_str().to_string();
        runner
    }

    #[must_use]
    pub fn new_custom(
        model: ModelHandle,
        instructions: String,
        tools: Vec<Arc<dyn EngineTool>>,
        max_turns: usize,
    ) -> Self {
        Self {
            model,
            instructions,
            tools,
            max_turns,
            ctx: TurnToolContext::default(),
            label: "subagent".to_string(),
            monitor_group: DEFAULT_MONITOR_GROUP.to_string(),
            monitor_label: "subagent".to_string(),
            position: None,
            parent_channel: false,
            isolated: false,
            inbox: None,
        }
    }

    /// Lets the subagent pause with a question for the agent that started it (the `ask_parent` tool).
    #[must_use]
    pub fn with_parent_channel(mut self) -> Self {
        self.parent_channel = true;
        self
    }

    /// Runs the agent in `root` (its worktree): file tools, the environment it is told about, and
    /// permission checks all treat `root` as the workspace.
    #[must_use]
    pub fn in_worktree(mut self, root: &std::path::Path) -> Self {
        self.isolated = true;
        self.tools = crate::tools::builtin_tools::rerooted(&self.tools, root);
        if let Some(project) = &self.ctx.project {
            let mut moved = (**project).clone();
            moved.current_dir = root.to_path_buf();
            self.ctx.project = Some(Arc::new(moved));
        }
        if let Some(hook) = &self.ctx.permission {
            self.ctx.permission = Some(Arc::new(hook.for_working_dir(root.to_path_buf())));
        }
        self
    }

    #[must_use]
    pub fn with_label(mut self, name: &str) -> Self {
        self.label = format!("subagent {name}");
        self.monitor_label = name.to_string();
        self
    }

    #[must_use]
    pub fn with_task_name(mut self, name: &str) -> Self {
        self.label = format!("subagent {name}");
        self.monitor_label = format!("{name} · {}", self.monitor_label);
        self
    }

    #[must_use]
    pub fn with_monitor_identity(mut self, group: &str, label: &str) -> Self {
        self.monitor_group = group.to_string();
        self.monitor_label = label.to_string();
        self
    }

    #[must_use]
    pub fn with_context(mut self, ctx: TurnToolContext) -> Self {
        self.ctx = ctx;
        self
    }

    #[must_use]
    pub(crate) fn with_progress(mut self, progress: rho_harness_core::presentation::ToolStreamPort) -> Self {
        self.ctx.progress = progress;
        self
    }

    /// Reads messages the delegating agent sends while the round runs.
    #[must_use]
    pub(crate) fn with_inbox(mut self, inbox: super::background::Inbox) -> Self {
        self.inbox = Some(inbox);
        self
    }

    /// Stops the round, and its tools, when `stop` fires instead of the turn's own signal.
    #[must_use]
    pub(crate) fn with_cancellation(mut self, stop: crate::engine::runner::turn::types::CancellationSignal) -> Self {
        self.ctx.cancellation = Some(stop);
        self
    }

    /// Adds the delegating agent's queued messages to the conversation; `true` if there were any.
    fn deliver_messages(&self, state: &mut RunState) -> bool {
        let messages = self
            .inbox
            .as_ref()
            .map(super::background::Inbox::take)
            .unwrap_or_default();
        for message in &messages {
            if let Some(tracker) = &state.tracker {
                tracker.event(AgentEvent::Task(message.clone()));
            }
            state.messages.push(ChatMessage::user(format!(
                "Message from the delegating agent:\n{}",
                message.trim()
            )));
        }
        !messages.is_empty()
    }

    #[must_use]
    pub fn with_position(mut self, position: impl Into<String>) -> Self {
        self.position = Some(position.into());
        self
    }

    /// Layers a persona's behavior instructions after the role or agent instructions.
    #[must_use]
    pub(crate) fn with_persona(mut self, persona: &rho_harness_core::agents::PersonaMetadata) -> Self {
        self.instructions = format!(
            "{}\n\n## Persona: {}\n{}",
            self.instructions.trim(),
            persona.name,
            persona.instructions.trim()
        );
        self
    }

    fn preamble(&self) -> String {
        let mut preamble = format!("{}\n\n", self.instructions.trim());
        if let Some(project) = &self.ctx.project {
            let names: Vec<String> = self.tools.iter().map(|t| t.name().to_string()).collect();
            preamble.push_str(&crate::engine::context::build_agent_grounding(project, &names));
            preamble.push_str("\n\n");
        }
        if let Some(position) = &self.position {
            preamble.push_str(&format!("You are running as step `{position}` of a workflow.\n\n"));
        }
        if self.parent_channel {
            preamble.push_str(ASK_PARENT_GUIDANCE);
        }
        preamble.push_str(&format!(
            "You have a budget of {} turns. Use early turns for targeted tool calls, and reserve remaining turns to synthesize your final answer.\n\nUnless your task asks for a specific format, end with a final answer containing: a one-paragraph summary, findings with file:line references, files changed (if any), and open questions.",
            self.max_turns
        ));
        preamble
    }

    pub(crate) fn model_label(&self) -> String {
        format!("{}/{}", self.model.provider_name(), self.model.model_name())
    }

    pub(crate) fn model(&self) -> &ModelHandle {
        &self.model
    }

    fn is_cancelled(&self) -> bool {
        self.ctx.cancellation.as_ref().is_some_and(|c| c.is_cancelled())
    }

    /// Resolves to `None` when the operator cancels before `fut` completes.
    async fn cancellable<T>(&self, fut: impl std::future::Future<Output = T>) -> Option<T> {
        let Some(cancel) = &self.ctx.cancellation else {
            return Some(fut.await);
        };
        tokio::select! {
            value = fut => Some(value),
            () = cancel.wait_cancelled() => None,
        }
    }

    pub async fn run(&self, prompt: &str, context_slice: Option<&str>) -> ToolResult {
        let task = build_subagent_prompt(prompt, context_slice);
        let round = self.run_round(Vec::new(), RoundInput::Prompt(&task)).await;
        match round.body() {
            Ok(body) => ToolResult::success(format!(
                "{}{}",
                Self::with_evidence(&body, &round),
                self.footer(&round, "")
            )),
            Err(error) => ToolResult::error(error),
        }
    }

    /// The directory tool paths are relative to.
    pub(crate) fn workspace_root(&self) -> std::path::PathBuf {
        super::workspace::root_of(&self.ctx)
    }

    /// Workspace snapshots run only in real sessions, which have a project directory.
    async fn snapshot(&self, root: &std::path::Path, previous: Option<&Snapshot>) -> Option<Snapshot> {
        match self.ctx.project {
            Some(_) => Snapshot::take(root, previous).await,
            None => None,
        }
    }

    /// Round-level snapshots look for changes made outside the agent's tools, which a worktree rules out.
    async fn round_snapshot(&self, root: &std::path::Path, previous: Option<&Snapshot>) -> Option<Snapshot> {
        if self.isolated {
            return None;
        }
        self.snapshot(root, previous).await
    }

    pub(crate) fn with_evidence(body: &str, round: &Round) -> String {
        let evidence = evidence(
            &round.changed_files,
            &round.workspace_changes,
            round.workspace_complete,
            &round.tool_log,
        );
        if evidence.is_empty() {
            body.to_string()
        } else {
            format!("{body}\n\n{evidence}")
        }
    }

    /// The run footer, e.g. `[subagent audit: 3 turns, 5 tool calls, 900 tokens, acme/smol]`; `detail` follows the name.
    pub(crate) fn footer(&self, round: &Round, detail: &str) -> String {
        let incomplete = if round.is_incomplete() { "incomplete, " } else { "" };
        let problems = super::evidence::failed_calls_text(round.problem_count())
            .map(|text| format!("{text}, "))
            .unwrap_or_default();
        format!(
            "{RUN_FOOTER_PREFIX}{}{detail}: {incomplete}{problems}{} turns, {} tool calls, {} tokens, {}]",
            self.label.strip_prefix("subagent").unwrap_or_default(),
            round.turns,
            round.tool_calls,
            round.tokens,
            self.model_label()
        )
    }

    /// Continues `transcript` from `input` until the round ends, closing any question the subagent left open.
    pub async fn run_round(&self, mut transcript: Vec<ChatMessage>, input: RoundInput<'_>) -> Round {
        let (text, task) = match input {
            RoundInput::Reply(text) if answer_pending_questions(&mut transcript, text) => (text, String::new()),
            RoundInput::Reply(text) => (text, text.trim().to_string()),
            RoundInput::Prompt(text) => {
                answer_pending_questions(&mut transcript, NO_REPLY);
                (text, text.trim().to_string())
            }
        };
        let root = self.workspace_root();
        let before = self.round_snapshot(&root, None).await;
        let mut complete = true;
        let mut state = RunState {
            task_pending: !task.is_empty(),
            task,
            messages: transcript,
            usage: StructuralUsage::default(),
            turns: 0,
            tool_calls: 0,
            tracker: self.start_tracking(text),
            changed_files: BTreeSet::new(),
            touched_files: BTreeSet::new(),
            tool_log: Vec::new(),
            bash_changes: BTreeMap::new(),
            root,
            question: None,
        };
        let end = if state.task_pending || !state.messages.is_empty() {
            self.run_turns(&mut state).await
        } else {
            RoundEnd::Failed("subagent prompt cannot be empty".to_string())
        };
        if let Some(tracker) = &state.tracker {
            tracker.finish(self.final_status(&end));
        }
        let after = self.round_snapshot(&state.root, before.as_ref()).await;
        let workspace_changes = match (&before, &after) {
            (Some(before), Some(after)) => {
                let changes = after.changes_since(before, &state.root);
                complete = changes.complete;
                changes
                    .files
                    .into_iter()
                    .map(|mut change| {
                        change.by = state.bash_changes.get(&change.path).cloned();
                        change
                    })
                    .collect()
            }
            _ => bash_changes(&state),
        };
        Round {
            end,
            transcript: state.messages,
            turns: state.turns,
            tool_calls: state.tool_calls,
            tokens: state.usage.input_tokens + state.usage.output_tokens,
            changed_files: state.changed_files,
            workspace_changes,
            workspace_complete: complete,
            tool_log: state.tool_log,
            touched_files: state.touched_files,
        }
    }

    fn start_tracking(&self, input: &str) -> Option<AgentHandle> {
        self.ctx.monitor.as_ref().map(|monitor| {
            let handle = monitor.start(
                &self.monitor_group,
                &self.monitor_label,
                &self.model_label(),
                self.max_turns,
            );
            handle.event(AgentEvent::Task(input.to_string()));
            handle
        })
    }

    fn tool_definitions(&self) -> Vec<ModelToolDefinition> {
        let mut defs: Vec<ModelToolDefinition> = self
            .tools
            .iter()
            .map(|t| ModelToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect();
        if self.parent_channel {
            defs.push(ask_parent_definition());
        }
        defs
    }

    async fn run_turns(&self, state: &mut RunState) -> RoundEnd {
        let preamble = self.preamble();
        let tools_def = self.tool_definitions();
        for turn in 1..=self.max_turns {
            if let Some(end) = self.step(turn, &preamble, &tools_def, state).await {
                return end;
            }
        }
        self.salvage(&format!("reached the turn limit of {}", self.max_turns), state)
            .await
    }

    fn final_status(&self, end: &RoundEnd) -> AgentStatus {
        match end {
            RoundEnd::Answered(_) => AgentStatus::Done,
            RoundEnd::Asked(_) | RoundEnd::Stopped { .. } => AgentStatus::InputRequired,
            RoundEnd::Partial(_) | RoundEnd::Failed(_) => AgentStatus::Failed,
            RoundEnd::Cancelled => AgentStatus::Cancelled,
        }
    }

    fn record_input(state: &mut RunState) {
        if state.task_pending {
            state.messages.push(ChatMessage::user(state.task.clone()));
            state.task_pending = false;
        }
    }

    /// Runs one model turn; `Some` ends the run.
    async fn step(
        &self,
        turn: usize,
        preamble: &str,
        tools_def: &[ModelToolDefinition],
        state: &mut RunState,
    ) -> Option<RoundEnd> {
        if self.is_cancelled() {
            return Some(interrupted());
        }
        if !state.task_pending {
            self.deliver_messages(state);
        }
        let req = ModelCompletionRequest {
            preamble: Some(preamble.to_string()),
            prompt: if state.task_pending {
                state.task.clone()
            } else {
                String::new()
            },
            messages: state.messages.clone(),
            tools: tools_def.to_vec(),
            max_tokens: None,
            temperature: Some(0.0),
        };
        let resp = match self.cancellable(self.model.complete(req)).await {
            None => return Some(interrupted()),
            Some(Err(e)) => {
                Self::record_input(state);
                return Some(Self::partial(
                    &format!("stopped after a model call failed ({e})"),
                    state,
                ));
            }
            Some(Ok(resp)) => resp,
        };
        state.usage = state.usage.merge(&resp.usage);
        state.turns = turn;
        track_response(state.tracker.as_ref(), turn, &resp.content, &resp.tool_calls);
        Self::record_input(state);

        if resp.tool_calls.is_empty() {
            if resp.content.trim().is_empty() {
                return Some(self.salvage("ended without writing a final answer", state).await);
            }
            state.messages.push(ChatMessage::assistant(resp.content.trim()));
            if turn < self.max_turns && self.deliver_messages(state) {
                return None;
            }
            return Some(RoundEnd::Answered(resp.content));
        }
        self.report_progress(turn, &resp.tool_calls);
        state
            .messages
            .push(assistant_tool_message(&resp.content, &resp.tool_calls));
        if let Err(end) = self.dispatch(&resp.tool_calls, state).await {
            return Some(end);
        }
        state.question.take().map(RoundEnd::Asked)
    }

    fn report_progress(&self, turn: usize, calls: &[ToolCall]) {
        let names: Vec<&str> = calls.iter().map(|c| c.function.name.as_str()).collect();
        self.ctx.progress.stream_chunk(&format!(
            "{} turn {turn}/{}: {}\n",
            self.label,
            self.max_turns,
            names.join(", ")
        ));
    }

    fn partial(reason: &str, state: &RunState) -> RoundEnd {
        let result = partial_result(reason, &state.messages);
        if result.is_error {
            RoundEnd::Failed(result.content)
        } else {
            RoundEnd::Partial(result.content)
        }
    }

    /// Ends a round that stopped before a final answer, asking the model to summarize its work so far.
    async fn salvage(&self, reason: &str, state: &mut RunState) -> RoundEnd {
        if self.is_cancelled() {
            return interrupted();
        }
        let synthesis = self
            .cancellable(attempt_recovery_synthesis(
                &self.model,
                &self.instructions,
                &state.messages,
            ))
            .await;
        match synthesis {
            None => interrupted(),
            Some(Some(summary)) => {
                let kept = without_unanswered_tool_calls(&state.messages).len();
                state.messages.truncate(kept);
                state.messages.push(ChatMessage::assistant(summary.clone()));
                RoundEnd::Stopped {
                    reason: reason.to_string(),
                    summary,
                }
            }
            Some(None) => Self::partial(reason, state),
        }
    }

    /// Records an `ask_parent` call's question; such calls stay open until the parent replies.
    fn take_question(&self, call: &ToolCall, state: &mut RunState) -> bool {
        if !self.parent_channel || call.function.name != ASK_PARENT_TOOL {
            return false;
        }
        let question = call.function.arguments["question"].as_str().unwrap_or_default().trim();
        let fallback = "(the subagent asked without a question)".to_string();
        state.question = Some(question.to_string())
            .filter(|q| !q.is_empty())
            .or(state.question.take())
            .or(Some(fallback));
        true
    }

    /// Applies hooks and permissions to every call in order, then executes the approved ones.
    async fn dispatch(&self, calls: &[ToolCall], state: &mut RunState) -> Result<(), RoundEnd> {
        let mut slots: Vec<Option<ChatMessage>> = vec![None; calls.len()];
        let mut approved: Vec<(usize, &ToolCall, Value)> = Vec::new();
        for (index, call) in calls.iter().enumerate() {
            if self.take_question(call, state) {
                continue;
            }
            let decision = self
                .cancellable(evaluate_tool_dispatch(
                    call,
                    self.ctx.lifecycle.as_ref(),
                    self.ctx.permission.as_ref(),
                ))
                .await
                .ok_or_else(interrupted)?;
            match decision {
                DispatchDecision::Proceed(args) => approved.push((index, call, args)),
                DispatchDecision::Deny(msg) => {
                    let reason = tool_result_text(&msg);
                    state.tool_log.push(ToolOutcome::new(
                        &call.function.name,
                        &call.function.arguments,
                        OutcomeStatus::Denied,
                        &reason,
                    ));
                    slots[index] = Some(msg);
                }
                DispatchDecision::Stop(_, reason) => {
                    return Err(RoundEnd::Failed(format!("Subagent stopped by hook: {reason}")));
                }
            }
        }

        let concurrent = approved.len() > 1
            && approved
                .iter()
                .all(|(_, call, _)| READ_ONLY_TOOLS.contains(&call.function.name.as_str()));
        let tracker = state.tracker.as_ref();
        let outcomes = if concurrent {
            futures::future::join_all(
                approved
                    .iter()
                    .map(|(_, call, args)| self.run_call(call, args, tracker)),
            )
            .await
        } else {
            let mut sequential = Vec::with_capacity(approved.len());
            for (_, call, args) in &approved {
                sequential.push(self.run_call(call, args, tracker).await);
            }
            sequential
        };

        let mut stop = None;
        for ((index, call, args), outcome) in approved.iter().zip(outcomes) {
            Self::record_outcome(state, &call.function.name, args, &outcome);
            slots[*index] = Some(outcome.message);
            stop = stop.or(outcome.stop);
        }
        state.tool_calls += calls.len();
        state.messages.extend(slots.into_iter().flatten());
        match stop {
            Some(reason) => Err(RoundEnd::Failed(format!("Subagent stopped by hook: {reason}"))),
            None => Ok(()),
        }
    }

    fn record_outcome(state: &mut RunState, name: &str, args: &Value, outcome: &CallOutcome) {
        let status = if outcome.is_error {
            OutcomeStatus::Error
        } else {
            OutcomeStatus::Ok
        };
        let mut logged = ToolOutcome::new(name, args, status, &outcome.output);
        let path = args["path"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .map(|p| normalize(&state.root, p));
        if let Some(path) = &path {
            logged.target.clone_from(path);
        }
        state.tool_log.push(logged);
        let command = args["command"].as_str().map(|c| super::evidence::clip(c, 60));
        for path in &outcome.bash_changes {
            state
                .bash_changes
                .insert(path.clone(), command.clone().unwrap_or_default());
        }
        state.touched_files.extend(outcome.bash_reads.iter().cloned());
        let Some(path) = path else {
            return;
        };
        if FILE_READING_TOOLS.contains(&name) {
            state.touched_files.insert(path.clone());
        }
        if !outcome.is_error && FILE_CHANGING_TOOLS.contains(&name) {
            state.changed_files.insert(path);
        }
    }

    async fn run_call(&self, call: &ToolCall, args: &Value, tracker: Option<&AgentHandle>) -> CallOutcome {
        let name = call.function.name.as_str();
        let bash_command = (name == "bash").then(|| args["command"].as_str().unwrap_or_default());
        let root = self.workspace_root();
        let before = match bash_command {
            Some(_) => self.snapshot(&root, None).await,
            None => None,
        };
        let (mut output, is_error) = self
            .cancellable(self.run_tool(name, args))
            .await
            .unwrap_or_else(|| ("Cancelled by operator".to_string(), true));
        let bash_changes = match &before {
            Some(before) => self
                .snapshot(&root, Some(before))
                .await
                .map(|after| {
                    after
                        .changes_since(before, &root)
                        .files
                        .into_iter()
                        .map(|c| c.path)
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let bash_reads = match bash_command {
            Some(command) => bash_file_arguments(&root, command).await,
            None => Vec::new(),
        };
        let stop = apply_tool_outcome_hooks(self.ctx.lifecycle.as_ref(), name, args, &mut output, is_error).await;
        let output = truncate_output(output);
        if let Some(tracker) = tracker {
            tracker.event(AgentEvent::ToolResult {
                name: name.to_string(),
                output: output.clone(),
                is_error,
            });
        }
        CallOutcome {
            message: tool_result_message(call, output.clone()),
            stop,
            is_error,
            output,
            bash_changes,
            bash_reads,
        }
    }

    async fn run_tool(&self, name: &str, args: &Value) -> (String, bool) {
        let Some(tool) = self.tools.iter().find(|t| t.name() == name) else {
            return (format!("Error: Unknown tool '{name}'"), true);
        };
        let tool = tool.bind_turn(&self.ctx).unwrap_or_else(|| tool.clone());
        match tool.execute(args.clone()).await {
            Ok(res) => (res.content, res.is_error),
            Err(e) => (format!("Error: {e}"), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_formatting() {
        assert_eq!(build_subagent_prompt("do work", None), "do work");
        assert_eq!(build_subagent_prompt("do work", Some("")), "do work");
        assert_eq!(
            build_subagent_prompt("do work", Some("important background")),
            "Context:\nimportant background\n\nTask:\ndo work"
        );
    }

    #[test]
    fn truncate_output_caps_length_on_char_boundaries() {
        let short = "héllo".to_string();
        assert_eq!(truncate_output(short.clone()), short);

        let long = "é".repeat(MAX_TOOL_OUTPUT_CHARS + 5);
        let cut = truncate_output(long);
        assert!(cut.ends_with("\n[truncated 5 characters]"));
        assert_eq!(cut.chars().filter(|c| *c == 'é').count(), MAX_TOOL_OUTPUT_CHARS);
    }
}
