//! A subagent round: the subagent's instructions, tools, and budget run on the main agent's turn
//! loop, with its conversation in memory and its output shown in its monitor record. The round
//! adds what a delegated task needs around that loop: evidence of what its tools did, a summary
//! when it stops early, and a pause when it asks the delegating agent a question.

use super::evidence::{ToolOutcome, evidence, problem_count};
use super::observed::{AskParentTool, RecordedTool, Recorder, take_question};
use super::presenter::AgentPresenter;
use super::recovery::{attempt_recovery_synthesis, partial_result};
use super::snapshot::Snapshot;
use super::workspace::WorkspaceChange;
use crate::engine::compactor::compact_messages;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::RunTracker;
use crate::engine::runner::history::without_unanswered_tool_calls;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use crate::engine::runner::turn::MemoryStore;
use crate::engine::runner::turn::runner::{
    AgentRunner, EMPTY_RESPONSE_ERROR, MultiTurnStreamItem, StreamEvent, ToolObserver,
};
use crate::engine::runner::turn::types::CancellationSignal;
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use futures::StreamExt;
use rho_harness_core::agents::{AgentEvent, AgentHandle, AgentStatus};
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ChatMessage, UserContent};
use rho_harness_core::presentation::Presenter;
use std::collections::BTreeSet;
use std::sync::Arc;

pub(crate) use super::observed::ASK_PARENT_TOOL;

const MAX_PARENT_MESSAGES: usize = 20;
const MAX_PARENT_MESSAGE_CHARS: usize = 1_500;
pub(crate) const RUN_FOOTER_PREFIX: &str = "\n\n[subagent";
const DEFAULT_MONITOR_GROUP: &str = "subagents";
pub(crate) const READ_ONLY_TOOLS: [&str; 5] = ["read", "fd", "rg", "web_fetch", "web_search"];
const ASK_PARENT_GUIDANCE: &str = "If you cannot proceed without a decision or information only the delegating agent has, call ask_parent with a specific question instead of guessing; your work so far is kept while you wait.\n\n";
const REPLY_HEADING: &str = "Reply from the delegating agent";
/// Times one round may compact its conversation after overflowing the model's context.
const MAX_COMPACTIONS: usize = 3;

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
    /// The transcript was compacted, so it replaces the saved conversation instead of extending it.
    pub compacted: bool,
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

/// What a round starts from: a new instruction, or a reply to the subagent's `ask_parent` question.
#[derive(Debug, Clone, Copy)]
pub enum RoundInput<'a> {
    Prompt(&'a str),
    Reply(&'a str),
}

/// Whether the conversation's last assistant message asked the delegating agent a question.
fn ended_with_question(transcript: &[ChatMessage]) -> bool {
    let last_assistant = transcript
        .iter()
        .rev()
        .find(|m| matches!(m, ChatMessage::Assistant { .. }));
    let Some(ChatMessage::Assistant { content, .. }) = last_assistant else {
        return false;
    };
    content
        .iter()
        .any(|c| matches!(c, AssistantContent::ToolCall(call) if call.function.name == ASK_PARENT_TOOL))
}

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
    inbox: Option<crate::mailbox::Mailbox>,
    /// The saved task the round belongs to, shown with it in the agent view.
    task_id: Option<String>,
    /// Recent conversation a compaction keeps word for word.
    keep_recent_tokens: usize,
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

/// How the turn loop finished: with the model's final text, stopped by the round's stop signal, or
/// with an error.
enum Finish {
    Done(String),
    Stopped,
    Failed(AppError),
}

/// What the round's turns added up to.
#[derive(Default)]
struct Tally {
    turns: usize,
    tool_calls: usize,
    tokens: u64,
}

/// A round in progress across runs of the turn loop (one per compaction).
struct RoundState {
    run: Run,
    owner: String,
    presenter: Arc<dyn Presenter>,
    question_tool: Option<Arc<dyn EngineTool>>,
    compacted: bool,
}

/// Where the next run of the turn loop starts: the conversation so far, the prompt to add (empty
/// when continuing after a compaction), and the totals of the runs before it.
struct Run {
    transcript: Vec<ChatMessage>,
    prompt: String,
    tally: Tally,
}

/// What one run of the turn loop is built from.
struct AgentSetup<'a> {
    store: &'a MemoryStore,
    sink: &'a Arc<TerminalApprovalSink>,
    prompt: String,
    turns_used: usize,
    recorder: &'a Arc<Recorder>,
    question_tool: Option<Arc<dyn EngineTool>>,
    stop: &'a CancellationSignal,
    /// The agent's monitor record, which its permission prompts mark as waiting for approval.
    tracker: Option<&'a AgentHandle>,
}

/// The finished turn loop: how it ended, the conversation it left, its totals, and whether that
/// conversation was compacted (so it replaces the saved one instead of extending it).
struct Drive {
    end: RoundEnd,
    transcript: Vec<ChatMessage>,
    tally: Tally,
    compacted: bool,
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
            task_id: None,
            keep_recent_tokens: rho_harness_core::tokens::DEFAULT_KEEP_RECENT_TOKENS,
        }
    }

    /// How much recent conversation a compaction keeps word for word.
    #[must_use]
    pub fn with_keep_recent_tokens(mut self, tokens: usize) -> Self {
        self.keep_recent_tokens = tokens;
        self
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

    /// Reads messages the delegating agent and the user send while the round runs.
    #[must_use]
    pub(crate) fn with_inbox(mut self, inbox: crate::mailbox::Mailbox) -> Self {
        self.inbox = Some(inbox);
        self
    }

    #[must_use]
    pub(crate) fn with_task_id(mut self, task_id: &str) -> Self {
        self.task_id = Some(task_id.to_string());
        self
    }

    /// Stops the round, and its tools, when `stop` fires instead of the turn's own signal.
    #[must_use]
    pub(crate) fn with_cancellation(mut self, stop: CancellationSignal) -> Self {
        self.ctx.cancellation = Some(stop);
        self
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

    /// Resolves to `None` when `stop` fires before `fut` completes.
    async fn unless_stopped<T>(stop: &CancellationSignal, fut: impl std::future::Future<Output = T>) -> Option<T> {
        tokio::select! {
            value = fut => Some(value),
            () = stop.wait_cancelled() => None,
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

    /// Round-level snapshots look for changes made outside the agent's tools, which a worktree rules
    /// out; they run only in real sessions, which have a project directory.
    async fn round_snapshot(&self, root: &std::path::Path, previous: Option<&Snapshot>) -> Option<Snapshot> {
        if self.isolated || self.ctx.project.is_none() {
            return None;
        }
        Snapshot::take(root, previous).await
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

    /// Continues `transcript` from `input` until the round ends.
    pub async fn run_round(&self, transcript: Vec<ChatMessage>, input: RoundInput<'_>) -> Round {
        let (shown, prompt) = match input {
            RoundInput::Reply(text) if ended_with_question(&transcript) => {
                (text, format!("{REPLY_HEADING}:\n{}", text.trim()))
            }
            RoundInput::Reply(text) | RoundInput::Prompt(text) => (text, text.trim().to_string()),
        };
        let root = self.workspace_root();
        let before = self.round_snapshot(&root, None).await;
        let tracker = self.start_tracking(shown);
        let recorder = Recorder::new(root.clone(), self.ctx.project.is_some());
        let drive = if prompt.is_empty() && transcript.is_empty() {
            Drive {
                end: RoundEnd::Failed("subagent prompt cannot be empty".to_string()),
                transcript,
                tally: Tally::default(),
                compacted: false,
            }
        } else {
            self.drive(transcript, prompt, tracker.as_ref(), &recorder).await
        };
        if let Some(tracker) = &tracker {
            tracker.finish(self.final_status(&drive.end));
        }
        let evidence = recorder.take();
        let after = self.round_snapshot(&root, before.as_ref()).await;
        let (workspace_changes, workspace_complete) = match (&before, &after) {
            (Some(before), Some(after)) => {
                let changes = after.changes_since(before, &root);
                let files = changes
                    .files
                    .into_iter()
                    .map(|mut change| {
                        change.by = evidence.bash_changes.get(&change.path).cloned();
                        change
                    })
                    .collect();
                (files, changes.complete)
            }
            _ => (evidence.bash_workspace_changes(&root), true),
        };
        Round {
            end: drive.end,
            transcript: drive.transcript,
            turns: drive.tally.turns,
            tool_calls: drive.tally.tool_calls,
            tokens: drive.tally.tokens,
            changed_files: evidence.changed_files,
            workspace_changes,
            workspace_complete,
            tool_log: evidence.tool_log,
            touched_files: evidence.touched_files,
            compacted: drive.compacted,
        }
    }

    /// Runs the subagent's turns on the shared turn loop and turns how it ended into a round end. When
    /// a turn overflows the model's context, the older conversation is compacted into a summary and the
    /// round continues with the turns it has left.
    async fn drive(
        &self,
        transcript: Vec<ChatMessage>,
        prompt: String,
        tracker: Option<&AgentHandle>,
        recorder: &Arc<Recorder>,
    ) -> Drive {
        let stop = self.ctx.cancellation.clone().unwrap_or_default();
        if stop.is_cancelled() {
            return Drive {
                end: RoundEnd::Cancelled,
                transcript,
                tally: Tally::default(),
                compacted: false,
            };
        }
        let presenter: Arc<dyn Presenter> = Arc::new(AgentPresenter::new(tracker.cloned(), self.ctx.progress.clone()));
        let (question_tool, questions) = AskParentTool::with_question_slot();
        let mut round = RoundState {
            run: Run {
                transcript,
                prompt,
                tally: Tally::default(),
            },
            owner: self.owner(),
            presenter,
            question_tool: self.parent_channel.then_some(question_tool),
            compacted: false,
        };
        let mut compactions = 0;
        let (finish, mut transcript) = loop {
            let (finish, messages) = self.run_once(&mut round, tracker, recorder, &stop).await;
            if !matches!(finish, Finish::Failed(AppError::ContextOverflow(_))) || compactions == MAX_COMPACTIONS {
                break (finish, messages);
            }
            match self.compacted(&messages).await {
                Some(compacted) => {
                    compactions += 1;
                    round.compacted = true;
                    round.run.transcript = compacted;
                }
                None => break (finish, messages),
            }
        };
        crate::tools::jobs::JobManager::global().cancel_owner(&round.owner);
        if !round.run.prompt.is_empty() {
            transcript.push(ChatMessage::user(round.run.prompt.clone()));
        }
        let end = match finish {
            Finish::Stopped => RoundEnd::Cancelled,
            Finish::Done(text) => match take_question(&questions) {
                Some(question) => RoundEnd::Asked(question),
                None if text.trim().is_empty() => {
                    self.salvage("ended without writing a final answer", &mut transcript, &stop)
                        .await
                }
                None => RoundEnd::Answered(text),
            },
            Finish::Failed(error) => self.ended_by(error, &mut transcript, &stop).await,
        };
        Drive {
            end,
            transcript,
            tally: round.run.tally,
            compacted: round.compacted,
        }
    }

    /// Runs the turn loop once from where `round` left off. A prompt the loop never saved (it failed
    /// before its first reply) stays in `round` for a retry and is added to the returned conversation.
    async fn run_once(
        &self,
        round: &mut RoundState,
        tracker: Option<&AgentHandle>,
        recorder: &Arc<Recorder>,
        stop: &CancellationSignal,
    ) -> (Finish, Vec<ChatMessage>) {
        let started_with = round.run.transcript.len();
        let store = MemoryStore::new(round.owner.clone(), std::mem::take(&mut round.run.transcript));
        let sink = TerminalApprovalSink::with_session(
            &round.presenter,
            TerminalSinkConfig {
                model_label: self.model_label(),
                run_tracker: RunTracker::default(),
            },
            self.ctx.session.clone(),
        );
        let agent = self.agent(AgentSetup {
            store: &store,
            sink: &sink,
            prompt: round.run.prompt.clone(),
            turns_used: round.run.tally.turns,
            recorder,
            question_tool: round.question_tool.clone(),
            stop,
            tracker,
        });
        let finish = self.consume(agent, &sink, tracker, stop, &mut round.run.tally).await;
        let messages = store.messages();
        if messages.len() > started_with {
            round.run.prompt.clear();
        }
        (finish, messages)
    }

    /// `messages` compacted for another run, with the summary redacted like the session's.
    async fn compacted(&self, messages: &[ChatMessage]) -> Option<Vec<ChatMessage>> {
        let mut compacted = compact_messages(&self.model, messages, self.keep_recent_tokens).await?;
        if let (Some(session), Some(ChatMessage::System { content })) = (&self.ctx.session, compacted.first_mut()) {
            *content = session.redact_credentials(content);
        }
        Some(compacted)
    }

    /// Owns the round's background work and messages: its mailbox, or a one-off id without one.
    fn owner(&self) -> String {
        self.inbox.as_ref().map_or_else(
            || format!("subagent-{}", uuid::Uuid::new_v4()),
            |inbox| inbox.id().to_string(),
        )
    }

    /// The turn loop for this subagent: its tools recorded as evidence, its conversation in the setup's
    /// store, and the turns it has left.
    fn agent(&self, setup: AgentSetup<'_>) -> AgentRunner {
        let mut tools: Vec<Arc<dyn EngineTool>> = self
            .tools
            .iter()
            .map(|tool| RecordedTool::wrap(tool.clone(), setup.recorder))
            .collect();
        tools.extend(setup.question_tool);
        let observer: Arc<dyn ToolObserver> = setup.recorder.clone();
        AgentRunner::with_store(
            self.model.clone(),
            tools,
            Arc::new(setup.store.clone()),
            setup.sink.clone(),
            self.workspace_root(),
        )
        .session(self.ctx.session.clone())
        .preamble(self.preamble())
        .prompt(setup.prompt)
        .budget(self.max_turns.saturating_sub(setup.turns_used))
        .lifecycle(self.ctx.lifecycle.clone())
        .permission(self.ctx.permission.as_ref().map(|hook| match setup.tracker {
            Some(tracker) => Arc::new(hook.for_agent(tracker.clone())),
            None => hook.clone(),
        }))
        .stream_port(self.ctx.progress.clone())
        .inbox(self.inbox.clone())
        .cancellation(Some(setup.stop.clone()))
        .monitor(self.ctx.monitor.clone())
        .project_context(self.ctx.project.clone())
        .observer(Some(observer))
    }

    /// The round end for a turn loop that stopped with `error`.
    async fn ended_by(
        &self,
        error: AppError,
        transcript: &mut Vec<ChatMessage>,
        stop: &CancellationSignal,
    ) -> RoundEnd {
        match error {
            AppError::Cancelled(_) if stop.is_cancelled() => RoundEnd::Cancelled,
            AppError::Cancelled(reason) => RoundEnd::Failed(format!("Subagent stopped by hook: {reason}")),
            AppError::ModelBudgetExhausted { .. } => {
                self.salvage(
                    &format!("reached the turn limit of {}", self.max_turns),
                    transcript,
                    stop,
                )
                .await
            }
            AppError::Provider(message) if message == EMPTY_RESPONSE_ERROR => {
                self.salvage("ended without writing a final answer", transcript, stop)
                    .await
            }
            error => Self::partial(&format!("stopped after a model call failed ({error})"), transcript),
        }
    }

    /// Reads the turn loop's stream to its end: shows its output, releases its display barriers, and
    /// adds its turns, tool calls, and tokens to `tally`. Stops early when `stop` fires.
    async fn consume(
        &self,
        agent: AgentRunner,
        sink: &Arc<TerminalApprovalSink>,
        tracker: Option<&AgentHandle>,
        stop: &CancellationSignal,
        tally: &mut Tally,
    ) -> Finish {
        let mut stream = agent.stream();
        let mut pending_names: Vec<String> = Vec::new();
        let mut output = None;
        loop {
            let Some(item) = Self::unless_stopped(stop, stream.next()).await else {
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
                Some(Ok(item)) => self.show(item, sink, tracker, tally, &mut pending_names),
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
        pending_names: &mut Vec<String>,
    ) {
        match item {
            MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Text(text)) => sink.emit_text(&text),
            MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(text)) => sink.emit_reasoning(&text),
            MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Arguments { .. }) => sink.flush_reasoning(),
            MultiTurnStreamItem::ToolCall { tool_call } => {
                tally.tool_calls += 1;
                pending_names.push(tool_call.function.name);
            }
            MultiTurnStreamItem::ModelTurnRetried(retry) => {
                tally.tool_calls = tally.tool_calls.saturating_sub(retry.discarded_tool_calls);
                pending_names.clear();
                sink.flush_display();
            }
            MultiTurnStreamItem::CompletionCall(call) => {
                tally.turns += 1;
                tally.tokens += call.usage.input_tokens + call.usage.output_tokens;
                sink.flush_display();
                if let Some(tracker) = tracker {
                    tracker.turn(tally.turns);
                }
                if !pending_names.is_empty() {
                    self.report_progress(tally.turns, pending_names);
                    pending_names.clear();
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

    fn start_tracking(&self, input: &str) -> Option<AgentHandle> {
        self.ctx.monitor.as_ref().map(|monitor| {
            let handle = monitor.start(
                &self.monitor_group,
                &self.monitor_label,
                &self.model_label(),
                self.max_turns,
            );
            handle.event(AgentEvent::Task(input.to_string()));
            if let Some(task_id) = &self.task_id {
                handle.set_task(task_id);
            }
            handle
        })
    }

    fn final_status(&self, end: &RoundEnd) -> AgentStatus {
        match end {
            RoundEnd::Answered(_) => AgentStatus::Done,
            RoundEnd::Asked(_) | RoundEnd::Stopped { .. } => AgentStatus::InputRequired,
            RoundEnd::Partial(_) | RoundEnd::Failed(_) => AgentStatus::Failed,
            RoundEnd::Cancelled => AgentStatus::Cancelled,
        }
    }

    fn report_progress(&self, turn: usize, names: &[String]) {
        self.ctx.progress.stream_chunk(&format!(
            "{} turn {turn}/{}: {}\n",
            self.label,
            self.max_turns,
            names.join(", ")
        ));
    }

    fn partial(reason: &str, transcript: &[ChatMessage]) -> RoundEnd {
        let result = partial_result(reason, transcript);
        if result.is_error {
            RoundEnd::Failed(result.content)
        } else {
            RoundEnd::Partial(result.content)
        }
    }

    /// Ends a round that stopped before a final answer, asking the model to summarize its work so far.
    async fn salvage(&self, reason: &str, transcript: &mut Vec<ChatMessage>, stop: &CancellationSignal) -> RoundEnd {
        if stop.is_cancelled() {
            return RoundEnd::Cancelled;
        }
        let synthesis = Self::unless_stopped(
            stop,
            attempt_recovery_synthesis(&self.model, &self.instructions, transcript),
        )
        .await;
        match synthesis {
            None => RoundEnd::Cancelled,
            Some(Some(summary)) => {
                let kept = without_unanswered_tool_calls(transcript).len();
                transcript.truncate(kept);
                transcript.push(ChatMessage::assistant(summary.clone()));
                RoundEnd::Stopped {
                    reason: reason.to_string(),
                    summary,
                }
            }
            Some(None) => Self::partial(reason, transcript),
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
    fn a_reply_is_framed_as_one_only_after_a_question() {
        let asked = ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(rho_harness_core::model::ToolCall::new(
                "q1",
                rho_harness_core::model::ToolFunction::new(ASK_PARENT_TOOL, serde_json::json!({})),
            ))],
        };
        assert!(ended_with_question(&[ChatMessage::user("task"), asked.clone()]));
        assert!(!ended_with_question(&[asked, ChatMessage::assistant("answer")]));
        assert!(!ended_with_question(&[]));
    }
}
