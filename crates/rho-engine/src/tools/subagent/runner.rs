//! A subagent round: the subagent's instructions, tools, and budget run on the main agent's turn
//! loop, with its conversation in memory and its output shown in its monitor record. The round
//! adds what a delegated task needs around that loop: evidence of what its tools did, a summary
//! when it stops early, a pause when it asks the delegating agent a question, and the report it
//! finishes with through `complete_task`.

use super::complete::{CompleteTaskTool, take_report};
use super::evidence::evidence;
use super::lsp::{WorktreeLsp, WorktreeLspGuard};
use super::observed::{AskParentTool, RecordedTool, Recorder, take_question};
use super::presenter::AgentPresenter;
use super::recovery::{attempt_recovery_synthesis, partial_result};
use super::round::{Round, RoundEnd, RoundInput, build_subagent_prompt, ended_with_question};
use super::snapshot::Snapshot;
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::compactor::{MAX_COMPACTIONS_PER_TURN, compact_messages};
use crate::engine::metrics::RunTracker;
use crate::engine::runner::history::without_unanswered_tool_calls;
use crate::engine::runner::sink::{TerminalApprovalSink, TerminalSinkConfig};
use crate::engine::runner::turn::MemoryStore;
use crate::engine::runner::turn::runner::{
    AgentRunner, EMPTY_RESPONSE_ERROR, MultiTurnStreamItem, StreamEvent, ToolObserver,
};
use crate::engine::runner::turn::streaming_tool::StreamingToolTracker;
use crate::engine::runner::turn::types::{CancellationSignal, unless_cancelled};
use crate::engine::tracking::{ContextBudget, ContextPolicy};
use crate::mailbox::{AgentId, Mailbox};
use crate::tools::engine_tool::{EngineTool, TurnToolContext};
use crate::tools::subagent::types::SubagentRole;
use crate::tools::types::ToolResult;
use futures::StreamExt;
use rho_harness_core::agents::{AgentEvent, AgentHandle, AgentStatus};
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::presentation::Presenter;
use rho_harness_core::provider::ModelRoute;
use std::sync::Arc;

pub(crate) const RUN_FOOTER_PREFIX: &str = "\n\n[subagent";
const DEFAULT_MONITOR_GROUP: &str = "subagents";
pub(crate) const READ_ONLY_TOOLS: [&str; 6] = ["read", "fd", "rg", "lsp", "web_fetch", "web_search"];
const ASK_PARENT_GUIDANCE: &str = "If you cannot proceed without a decision or information only the delegating agent has, call ask_parent with a specific question instead of guessing; your work so far is kept while you wait.\n\n";
const REPLY_HEADING: &str = "Reply from the delegating agent";

pub fn filter_tools_for_role(tools: &[Arc<dyn EngineTool>], role: SubagentRole) -> Vec<Arc<dyn EngineTool>> {
    let allowed = role.allowed_tools();
    tools_named(tools, |name| allowed.contains(&name))
}

/// The tools whose names `keep` accepts, sorted by name so the tool list is stable across requests.
/// `lsp` is limited to queries unless the tools can change files.
pub(super) fn tools_named(tools: &[Arc<dyn EngineTool>], keep: impl Fn(&str) -> bool) -> Vec<Arc<dyn EngineTool>> {
    let mut kept: Vec<Arc<dyn EngineTool>> = tools.iter().filter(|t| keep(t.name())).cloned().collect();
    kept.sort_by(|a, b| a.name().cmp(b.name()));
    super::lsp::restrict_lsp(kept)
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
    inbox: Option<Mailbox>,
    /// The saved task the round belongs to, shown with it in the agent view.
    task_id: Option<String>,
    /// Recent conversation a compaction keeps word for word.
    keep_recent_tokens: usize,
    /// Where its model's window compacts; without one it compacts only after an overflow.
    context_policy: Option<ContextPolicy>,
    worktree_lsp: Option<WorktreeLsp>,
    /// The worktree whose language servers the round started.
    lsp_root: Option<std::path::PathBuf>,
}

/// How the turn loop finished: with the model's final text, stopped by the round's stop signal, or
/// with an error.
enum Finish {
    Done(String),
    Stopped,
    Failed(AppError),
}

/// What a round's stream has shown so far: the tools of the model call in progress, and the tool
/// call whose arguments are streaming into a preview card.
#[derive(Default)]
struct Shown {
    pending_names: Vec<String>,
    streaming_tool: StreamingToolTracker,
}

/// What the round's turns added up to.
#[derive(Default)]
struct Tally {
    turns: usize,
    tool_calls: usize,
    tokens: u64,
    cached_tokens: u64,
}

/// A round in progress across runs of the turn loop (one per compaction).
struct RoundState {
    run: Run,
    /// The round's own mailbox, which owns the background work it starts.
    inbox: Option<Mailbox>,
    /// Owns the round's conversation: its mailbox's id, or a one-off id without one.
    owner: String,
    presenter: Arc<dyn Presenter>,
    question_tool: Option<Arc<dyn EngineTool>>,
    report_tool: Arc<dyn EngineTool>,
    compacted: bool,
    /// The round's context budget, its notices carried across runs until a compaction starts a
    /// fresh one; its compaction stops are off once a stop found nothing to compact.
    context: Option<ContextBudget>,
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
    inbox: Option<Mailbox>,
    prompt: String,
    turns_used: usize,
    recorder: &'a Arc<Recorder>,
    question_tool: Option<Arc<dyn EngineTool>>,
    report_tool: Arc<dyn EngineTool>,
    stop: &'a CancellationSignal,
    /// The agent's monitor record, which its permission prompts mark as waiting for approval.
    tracker: Option<&'a AgentHandle>,
    context: Option<ContextBudget>,
}

/// The finished turn loop: how it ended, the conversation it left, its totals, and whether that
/// conversation was compacted (so it replaces the saved one instead of extending it).
struct Drive {
    end: RoundEnd,
    data: Option<serde_json::Value>,
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
            context_policy: None,
            worktree_lsp: None,
            lsp_root: None,
        }
    }

    #[must_use]
    pub fn with_worktree_lsp(mut self, open: Option<WorktreeLsp>) -> Self {
        self.worktree_lsp = open;
        self
    }

    #[cfg(test)]
    pub(crate) fn tool(&self, name: &str) -> Option<&Arc<dyn EngineTool>> {
        self.tools.iter().find(|t| t.name() == name)
    }

    /// Stops the language servers the round starts in its worktree, on every exit path including
    /// the round being dropped.
    pub(crate) fn worktree_lsp_guard(&self) -> WorktreeLspGuard {
        WorktreeLspGuard(self.lsp_root.clone())
    }

    /// How much recent conversation a compaction keeps word for word.
    #[must_use]
    pub fn with_keep_recent_tokens(mut self, tokens: usize) -> Self {
        self.keep_recent_tokens = tokens;
        self
    }

    #[must_use]
    pub fn with_context_policy(mut self, policy: Option<ContextPolicy>) -> Self {
        self.context_policy = policy;
        self
    }

    /// A fresh budget against this round's model.
    fn context_budget(&self) -> Option<ContextBudget> {
        let route = ModelRoute::new(self.model.provider_name(), self.model.model_name());
        Some(self.context_policy.as_ref()?.budget(&route))
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
        self.tools = crate::tools::builtin_tools::rerooted(&self.tools, root, self.worktree_lsp.as_ref());
        self.lsp_root = self.worktree_lsp.as_ref().map(|_| root.to_path_buf());
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
    pub(crate) fn with_inbox(mut self, inbox: Mailbox) -> Self {
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
            "You have a budget of {} turns. Use early turns for targeted tool calls, and reserve remaining turns to write your report.\n\nWhen the task is done, call complete_task with your full report; only that report reaches the delegating agent. Unless your task asks for a specific format, the report contains: a one-paragraph summary, findings with file:line references, files changed (if any), and open questions.",
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
    async fn round_snapshot(
        &self,
        root: &std::path::Path,
        previous: Option<&Snapshot>,
    ) -> Result<Option<Snapshot>, String> {
        if self.isolated || self.ctx.project.is_none() {
            return Ok(None);
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

    /// The run footer, e.g. `[subagent audit: 3 turns, 5 tool calls, 900 tokens (600 cached), acme/smol]`;
    /// `detail` follows the name.
    pub(crate) fn footer(&self, round: &Round, detail: &str) -> String {
        let incomplete = if round.is_incomplete() { "incomplete, " } else { "" };
        let problems = super::evidence::failed_calls_text(round.problem_count())
            .map(|text| format!("{text}, "))
            .unwrap_or_default();
        let cached = match round.cached_tokens {
            0 => String::new(),
            cached => format!(" ({cached} cached)"),
        };
        format!(
            "{RUN_FOOTER_PREFIX}{}{detail}: {incomplete}{problems}{} turns, {} tool calls, {} tokens{cached}, {}]",
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
                data: None,
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
        let previous = before.as_ref().ok().and_then(Option::as_ref);
        let after = self.round_snapshot(&root, previous).await;
        let (workspace_changes, workspace_complete) = match (&before, &after) {
            (Ok(Some(before)), Ok(Some(after))) => {
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
            (Err(error), _) | (_, Err(error)) => {
                tracing::warn!("subagent workspace snapshot failed: {error}");
                (evidence.bash_workspace_changes(&root), false)
            }
            _ => (evidence.bash_workspace_changes(&root), true),
        };
        Round {
            end: drive.end,
            data: drive.data,
            transcript: drive.transcript,
            turns: drive.tally.turns,
            tool_calls: drive.tally.tool_calls,
            tokens: drive.tally.tokens,
            cached_tokens: drive.tally.cached_tokens,
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
                data: None,
                transcript,
                tally: Tally::default(),
                compacted: false,
            };
        }
        let presenter: Arc<dyn Presenter> = Arc::new(AgentPresenter::new(tracker.cloned()));
        let (question_tool, questions) = AskParentTool::with_question_slot();
        let (report_tool, report) = CompleteTaskTool::with_report_slot();
        let inbox = self.round_inbox();
        let owner = inbox.as_ref().map_or_else(
            || format!("subagent-{}", uuid::Uuid::new_v4()),
            |inbox| inbox.id().to_string(),
        );
        let mut round = RoundState {
            run: Run {
                transcript,
                prompt,
                tally: Tally::default(),
            },
            inbox,
            owner,
            presenter,
            question_tool: self.parent_channel.then_some(question_tool),
            report_tool,
            compacted: false,
            context: self.context_budget(),
        };
        let mut compactions = 0;
        let (finish, mut transcript) = loop {
            let (finish, messages) = self.run_once(&mut round, tracker, recorder, &stop).await;
            if let Finish::Failed(error) = &finish
                && self.make_room(error, &messages, &mut round, &mut compactions).await
            {
                continue;
            }
            break (finish, messages);
        };
        if let Some(inbox) = &round.inbox {
            inbox.cancel_jobs();
        }
        if !round.run.prompt.is_empty() {
            transcript.push(ChatMessage::user(round.run.prompt.clone()));
        }
        let report = take_report(&report);
        let data = report.as_ref().and_then(|r| r.data.clone());
        let end = match finish {
            Finish::Stopped => RoundEnd::Cancelled,
            Finish::Done(reply) => {
                let question = take_question(&questions);
                let text = report.map(|r| match (r.text.is_empty(), r.data) {
                    (true, Some(data)) => data.to_string(),
                    _ => r.text,
                });
                self.answered(reply, question, text, &mut transcript, &stop).await
            }
            Finish::Failed(error) => self.ended_by(error, &mut transcript, &stop).await,
        };
        Drive {
            end,
            data,
            transcript,
            tally: round.run.tally,
            compacted: round.compacted,
        }
    }

    /// How a round whose turns finished ends: a question waits for its reply, a `complete_task`
    /// report is the answer, and otherwise the last reply is, unless it is empty.
    async fn answered(
        &self,
        reply: String,
        question: Option<String>,
        report: Option<String>,
        transcript: &mut Vec<ChatMessage>,
        stop: &CancellationSignal,
    ) -> RoundEnd {
        if let Some(question) = question {
            return RoundEnd::Asked(question);
        }
        match report {
            Some(report) => RoundEnd::Answered(report),
            None if reply.trim().is_empty() => {
                self.salvage("ended without writing a final answer", transcript, stop)
                    .await
            }
            None => RoundEnd::Answered(reply),
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
            inbox: round.inbox.clone(),
            prompt: round.run.prompt.clone(),
            turns_used: round.run.tally.turns,
            recorder,
            question_tool: round.question_tool.clone(),
            report_tool: round.report_tool.clone(),
            stop,
            tracker,
            context: round.context.clone(),
        });
        let finish = self.consume(agent, &sink, tracker, stop, &mut round.run.tally).await;
        let messages = store.messages();
        if messages.len() > started_with {
            round.run.prompt.clear();
        }
        (finish, messages)
    }

    /// Readies another run after `error` stopped one for a full context: compacted, or, when an
    /// early stop finds nothing to compact, unchanged and without early stops. False when the run
    /// cannot continue.
    async fn make_room(
        &self,
        error: &AppError,
        messages: &[ChatMessage],
        round: &mut RoundState,
        compactions: &mut usize,
    ) -> bool {
        if !error.needs_compaction() {
            return false;
        }
        let compacted = if *compactions < MAX_COMPACTIONS_PER_TURN {
            self.compacted(messages).await
        } else {
            None
        };
        match compacted {
            Some(compacted) => {
                *compactions += 1;
                round.compacted = true;
                round.run.transcript = compacted;
                round.context = self.context_budget();
                true
            }
            None if matches!(error, AppError::CompactionDue { .. }) => {
                round.context = round.context.take().map(|budget| budget.with_compaction_stops(false));
                round.run.transcript = messages.to_vec();
                true
            }
            None => false,
        }
    }

    /// `messages` compacted for another run, with the summary redacted like the session's.
    async fn compacted(&self, messages: &[ChatMessage]) -> Option<Vec<ChatMessage>> {
        let mut compacted = compact_messages(&self.model, messages, self.keep_recent_tokens).await?;
        if let (Some(session), Some(ChatMessage::System { content })) = (&self.ctx.session, compacted.first_mut()) {
            *content = session.redact_credentials(content);
        }
        Some(compacted)
    }

    /// The round's mailbox: the one it was given, or else a private one under the delegating agent's,
    /// so the background work it starts has an owner that waits for it and stops it.
    fn round_inbox(&self) -> Option<Mailbox> {
        self.inbox.clone().or_else(|| {
            let parent = self.ctx.inbox.as_ref()?;
            let id = AgentId::from(format!("subagent-{}", uuid::Uuid::new_v4()));
            Some(parent.registry().open(&id, Some(parent.id())))
        })
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
        tools.push(setup.report_tool);
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
        .inbox(setup.inbox)
        .cancellation(Some(setup.stop.clone()))
        .monitor(self.ctx.monitor.clone())
        .project_context(self.ctx.project.clone())
        .context_budget(setup.context)
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
        let synthesis = unless_cancelled(
            Some(stop),
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
