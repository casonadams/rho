use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::types::{CompletionCall, PromptResponse, StructuralUsage};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::runner::turn::types::{CancellationSignal, SteeringQueueProvider};
use crate::hook::LifecycleHook;
use crate::provider::adapter::{FinishReason, ModelCompletionRequest, ModelStreamEvent, ModelToolDefinition};
use crate::repeat::RepeatedCallTracker;
use crate::tools::EngineTool;
use crate::tools::engine_tool::TurnToolContext;
use futures::Stream;
use futures::StreamExt;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ChatMessage, Reasoning, ToolCall};
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::session::SessionManager;
use std::pin::Pin;
use std::sync::Arc;

mod dispatch;

pub use dispatch::DenialObserver;
use dispatch::{
    CallControl, ToolExecutionContext, batch_is_concurrency_safe, execute_concurrent_tool_calls,
    execute_single_tool_call,
};
#[cfg(test)]
use dispatch::{DispatchDecision, evaluate_tool_dispatch};

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Text(String),
    Reasoning(String),
    Arguments { id: String, json: String },
}

#[derive(Clone, Debug)]
pub enum MultiTurnStreamItem {
    StreamAssistantItem(StreamEvent),
    ToolCall {
        tool_call: ToolCall,
    },
    ToolExecutionCommitted,
    ModelTurnRetried(ModelRetry),
    CompletionCall(CompletionCall),
    FinalResponse(PromptResponse),
    DisplayBarrier(Arc<tokio::sync::Notify>),
    /// Messages from the agent's mailbox or the user's steering, as the model received them.
    MessagesDelivered(Vec<String>),
}

type TurnItem = Result<MultiTurnStreamItem, AppError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRetry {
    pub attempt: usize,
    pub max_attempts: usize,
    pub delay: std::time::Duration,
    pub reason: String,
    pub discarded_tool_calls: usize,
}

const MODEL_CALL_ATTEMPTS: usize = 3;
const MODEL_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

fn retry_delay(attempt: usize, error: &AppError) -> std::time::Duration {
    let backoff = std::time::Duration::from_secs(1 << attempt.min(5));
    error
        .retry_after()
        .map_or(backoff, |wait| wait.min(crate::provider::error::MAX_RETRY_AFTER))
}

struct TurnStream {
    rx: tokio::sync::mpsc::Receiver<TurnItem>,
    task: tokio::task::JoinHandle<()>,
}

impl Stream for TurnStream {
    type Item = TurnItem;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

impl Drop for TurnStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct TurnEmitter {
    tx: tokio::sync::mpsc::Sender<TurnItem>,
    cancellation: Option<CancellationSignal>,
}

impl TurnEmitter {
    fn ensure_live(&self) -> Result<(), AppError> {
        if self.tx.is_closed() || self.cancellation.as_ref().is_some_and(CancellationSignal::is_cancelled) {
            return Err(AppError::Cancelled("operator interrupt".to_string()));
        }
        Ok(())
    }

    async fn send(&self, item: MultiTurnStreamItem) -> Result<(), AppError> {
        self.ensure_live()?;
        self.tx
            .send(Ok(item))
            .await
            .map_err(|_| AppError::Cancelled("operator interrupt".to_string()))
    }

    /// Direct sink calls from the runner task must not overtake display items still queued
    /// for the stream consumer, or a tool card would split a half-rendered thinking block.
    async fn wait_for_display_drain(&self) -> Result<(), AppError> {
        let drained = Arc::new(tokio::sync::Notify::new());
        self.send(MultiTurnStreamItem::DisplayBarrier(Arc::clone(&drained)))
            .await?;
        drained.notified().await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct AgentRunner {
    model: ModelHandle,
    tools: Vec<Arc<dyn EngineTool>>,
    /// The session tools see; absent for an agent without one.
    session_manager: Option<SessionManager>,
    store: Arc<dyn super::store::TurnStore>,
    preamble: String,
    prompt: String,
    budget: usize,
    history: Vec<ChatMessage>,
    sink: Arc<TerminalApprovalSink>,
    repeat_tracker: RepeatedCallTracker,
    lifecycle: Option<Arc<LifecycleHook>>,
    permission: Option<Arc<crate::permission::PermissionHook>>,
    stream_port: Option<ToolStreamPort>,
    steering: Option<Arc<dyn SteeringQueueProvider>>,
    inbox: Option<crate::mailbox::Mailbox>,
    cancellation: Option<CancellationSignal>,
    on_denied: Option<DenialObserver>,
    working_dir: std::path::PathBuf,
    monitor: Option<rho_harness_core::agents::AgentMonitor>,
    project: Option<Arc<crate::engine::context::ProjectContext>>,
    worktrees: Option<crate::worktree::WorktreeStore>,
}

impl AgentRunner {
    pub fn new(
        model: ModelHandle,
        tools: Vec<Arc<dyn EngineTool>>,
        session_manager: SessionManager,
        sink: Arc<TerminalApprovalSink>,
    ) -> Self {
        let working_dir = session_manager
            .file_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        let store: Arc<dyn super::store::TurnStore> = Arc::new(session_manager.clone());
        Self::assemble(model, tools, store, sink, working_dir).session(Some(session_manager))
    }

    /// A runner whose conversation lives in `store` (an agent with no session file of its own),
    /// working in `working_dir`.
    pub fn with_store(
        model: ModelHandle,
        tools: Vec<Arc<dyn EngineTool>>,
        store: Arc<dyn super::store::TurnStore>,
        sink: Arc<TerminalApprovalSink>,
        working_dir: std::path::PathBuf,
    ) -> Self {
        Self::assemble(model, tools, store, sink, working_dir)
    }

    fn assemble(
        model: ModelHandle,
        tools: Vec<Arc<dyn EngineTool>>,
        store: Arc<dyn super::store::TurnStore>,
        sink: Arc<TerminalApprovalSink>,
        working_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            model,
            tools,
            store,
            session_manager: None,
            preamble: String::new(),
            prompt: String::new(),
            budget: 10,
            history: Vec::new(),
            sink,
            repeat_tracker: RepeatedCallTracker::new(working_dir.clone()),
            lifecycle: None,
            permission: None,
            stream_port: None,
            steering: None,
            inbox: None,
            cancellation: None,
            on_denied: None,
            working_dir,
            monitor: None,
            project: None,
            worktrees: None,
        }
    }

    pub fn preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = preamble.into();
        self
    }

    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    pub fn budget(mut self, budget: usize) -> Self {
        self.budget = budget;
        self
    }

    pub fn history(mut self, history: Vec<ChatMessage>) -> Self {
        self.history = history;
        self
    }

    /// Keeps the turn's conversation in `store` instead of the session, and makes the store's owner
    /// own the turn's background work. The session still reaches tools through their context.
    pub fn store(mut self, store: Arc<dyn super::store::TurnStore>) -> Self {
        self.store = store;
        self
    }

    /// The session the turn's tools see (where subagent tasks are saved).
    pub fn session(mut self, session: Option<SessionManager>) -> Self {
        self.session_manager = session;
        self
    }

    /// Tells `observer` about every call a hook or the permission policy refuses.
    pub fn on_denied(mut self, observer: Option<DenialObserver>) -> Self {
        self.on_denied = observer;
        self
    }

    #[cfg(test)]
    fn session_file(&self) -> &SessionManager {
        self.session_manager.as_ref().expect("test runners have a session")
    }

    pub fn lifecycle(mut self, lifecycle: Option<Arc<LifecycleHook>>) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    pub fn permission(mut self, permission: Option<Arc<crate::permission::PermissionHook>>) -> Self {
        self.permission = permission;
        self
    }

    pub fn stream_port(mut self, stream_port: ToolStreamPort) -> Self {
        self.stream_port = Some(stream_port);
        self
    }

    pub fn steering(mut self, steering: Option<Arc<dyn SteeringQueueProvider>>) -> Self {
        self.steering = steering;
        self
    }

    /// The agent's mailbox: read before each model call, and handed to its tools so the background
    /// work they start reports back to it.
    pub fn inbox(mut self, inbox: Option<crate::mailbox::Mailbox>) -> Self {
        self.inbox = inbox;
        self
    }

    pub fn cancellation(mut self, cancellation: Option<CancellationSignal>) -> Self {
        self.cancellation = cancellation;
        self
    }

    pub fn monitor(mut self, monitor: Option<rho_harness_core::agents::AgentMonitor>) -> Self {
        self.monitor = monitor;
        self
    }

    pub fn project_context(mut self, project: Option<Arc<crate::engine::context::ProjectContext>>) -> Self {
        self.project = project;
        self
    }

    /// Reminds the model, once per turn, to merge or discard the worktrees its session still owns.
    pub fn worktrees(mut self, store: Option<crate::worktree::WorktreeStore>) -> Self {
        self.worktrees = store;
        self
    }

    pub fn working_dir(mut self, working_dir: impl Into<std::path::PathBuf>) -> Self {
        self.working_dir = working_dir.into();
        self
    }

    fn parent_history_snapshot(&self) -> Arc<[ChatMessage]> {
        let mut snapshot: Vec<ChatMessage> = self.history.clone();
        if !self.prompt.trim().is_empty() {
            snapshot.push(ChatMessage::user(self.prompt.clone()));
        }
        snapshot.into()
    }

    pub fn stream(mut self) -> Pin<Box<dyn Stream<Item = Result<MultiTurnStreamItem, AppError>> + Send>> {
        let turn_ctx = TurnToolContext {
            permission: self.permission.clone(),
            lifecycle: self.lifecycle.clone(),
            cancellation: self.cancellation.clone(),
            progress: self.stream_port.clone().unwrap_or_default(),
            parent_history: self.parent_history_snapshot(),
            monitor: self.monitor.clone(),
            project: self.project.clone(),
            session: self.session_manager.clone(),
            inbox: self.inbox.clone(),
        };
        self.tools = self
            .tools
            .iter()
            .map(|tool| tool.bind_turn(&turn_ctx).unwrap_or_else(|| tool.clone()))
            .collect();
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let emitter = TurnEmitter {
            tx,
            cancellation: self.cancellation.clone(),
        };
        let task = tokio::spawn(async move {
            if let Err(error) = run_agent_loop(self, &emitter).await {
                let _ = emitter.tx.send(Err(error)).await;
            }
        });

        Box::pin(TurnStream { rx, task })
    }
}

#[derive(Default)]
struct TurnStreamOutcome {
    text: String,
    reasoning: String,
    content: Vec<AssistantContent>,
    tool_calls: Vec<ToolCall>,
    usage: StructuralUsage,
    finish_reason: Option<FinishReason>,
}

impl TurnStreamOutcome {
    fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
        if let Some(AssistantContent::Text(last)) = self.content.last_mut() {
            last.text.push_str(text);
        } else {
            self.content.push(AssistantContent::text(text));
        }
    }
}

async fn stream_single_event(
    event: ModelStreamEvent,
    outcome: &mut TurnStreamOutcome,
    emitter: &TurnEmitter,
) -> Result<(), AppError> {
    match event {
        ModelStreamEvent::Text(t) => {
            outcome.push_text(&t);
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Text(t)))
                .await
        }
        ModelStreamEvent::Reasoning(r) => {
            outcome.reasoning.push_str(&r);
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(r)))
                .await
        }
        ModelStreamEvent::ReasoningBlock(block) => {
            outcome.content.push(AssistantContent::Reasoning(block));
            Ok(())
        }
        ModelStreamEvent::ToolCall(tc) => {
            let args_str = serde_json::to_string(&tc.function.arguments).unwrap_or_default();
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Arguments {
                    id: tc.id.clone(),
                    json: args_str,
                }))
                .await?;
            emitter
                .send(MultiTurnStreamItem::ToolCall { tool_call: tc.clone() })
                .await?;
            outcome.content.push(AssistantContent::ToolCall(tc.clone()));
            outcome.tool_calls.push(tc);
            Ok(())
        }
        ModelStreamEvent::Usage(u) => {
            outcome.usage = outcome.usage.merge(&u);
            Ok(())
        }
        ModelStreamEvent::Finish(f) => {
            outcome.finish_reason = Some(f);
            Ok(())
        }
    }
}

async fn stream_turn(
    mut stream: Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>,
    emitter: &TurnEmitter,
    outcome: &mut TurnStreamOutcome,
) -> Result<(), AppError> {
    while let Some(res) = stream.next().await {
        stream_single_event(res?, outcome, emitter).await?;
    }
    Ok(())
}

async fn attempt_model_call(
    runner: &AgentRunner,
    req: ModelCompletionRequest,
    emitter: &TurnEmitter,
    outcome: &mut TurnStreamOutcome,
) -> Result<(), AppError> {
    let stream = tokio::time::timeout(MODEL_RESPONSE_TIMEOUT, runner.model.stream(req))
        .await
        .map_err(|_| {
            AppError::Network(format!(
                "model provider sent no response within {}s",
                MODEL_RESPONSE_TIMEOUT.as_secs()
            ))
        })??;
    stream_turn(stream, emitter, outcome).await
}

async fn sleep_unless_cancelled(delay: std::time::Duration, runner: &AgentRunner) -> Result<(), AppError> {
    tokio::select! {
        () = tokio::time::sleep(delay) => Ok(()),
        () = async {
            match runner.cancellation.as_ref() {
                Some(c) => c.wait_cancelled().await,
                None => std::future::pending::<()>().await,
            }
        } => Err(AppError::Cancelled("operator interrupt".to_string())),
    }
}

/// Retries one model request on transient failures so a dropped connection costs a request, not the whole turn.
async fn call_model(
    runner: &AgentRunner,
    req: &ModelCompletionRequest,
    emitter: &TurnEmitter,
) -> Result<TurnStreamOutcome, AppError> {
    let mut attempt = 1;
    loop {
        let mut outcome = TurnStreamOutcome::default();
        let error = match attempt_model_call(runner, req.clone(), emitter, &mut outcome).await {
            Ok(()) => return Ok(outcome),
            Err(error) if attempt < MODEL_CALL_ATTEMPTS && error.is_transient() => error,
            Err(error) => return Err(error),
        };
        let retry = ModelRetry {
            attempt,
            max_attempts: MODEL_CALL_ATTEMPTS,
            delay: retry_delay(attempt, &error),
            reason: error.to_string(),
            discarded_tool_calls: outcome.tool_calls.len(),
        };
        let delay = retry.delay;
        emitter.send(MultiTurnStreamItem::ModelTurnRetried(retry)).await?;
        sleep_unless_cancelled(delay, runner).await?;
        attempt += 1;
    }
}

fn append_notification_messages(
    notifications: Vec<String>,
    current_messages: &mut Vec<ChatMessage>,
    new_turn_messages: &mut Vec<ChatMessage>,
) {
    for notification in notifications {
        let notif_msg = ChatMessage::user(notification);
        current_messages.push(notif_msg.clone());
        new_turn_messages.push(notif_msg);
    }
}

/// Adds the agent's waiting messages to the turn: its mailbox (background notices, messages from
/// other agents and the user) and the user's queued steering. Returns whether any arrived.
async fn deliver_messages(runner: &AgentRunner, ctx: &mut RunnerStepContext<'_>, emitter: &TurnEmitter) -> bool {
    let mut messages: Vec<String> = Vec::new();
    if let Some(inbox) = &runner.inbox {
        let parent = inbox.parent();
        messages.extend(inbox.take().iter().map(|m| m.for_model(parent.as_deref())));
    }
    if let Some(provider) = &runner.steering {
        messages.extend(provider.poll_steering().await);
    }
    let delivered = !messages.is_empty();
    if delivered {
        let _ = emitter
            .send(MultiTurnStreamItem::MessagesDelivered(messages.clone()))
            .await;
    }
    append_notification_messages(messages, ctx.current_messages, ctx.new_turn_messages);
    delivered
}

/// Waits for a message, a cancellation, or a short tick (a job can end without sending anything).
async fn wait_for_message(runner: &AgentRunner) -> Result<(), AppError> {
    tokio::select! {
        () = async {
            match &runner.inbox {
                Some(inbox) => inbox.wait().await,
                None => std::future::pending::<()>().await,
            }
        } => Ok(()),
        () = async {
            match runner.cancellation.as_ref() {
                Some(c) => c.wait_cancelled().await,
                None => std::future::pending::<()>().await,
            }
        } => Err(AppError::Cancelled("operator interrupt".to_string())),
        () = tokio::time::sleep(std::time::Duration::from_millis(250)) => Ok(()),
    }
}

enum WaitStepResult {
    EventDelivered,
    ContinueWaiting,
}

async fn wait_step(
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<WaitStepResult, AppError> {
    if runner.cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
        return Err(AppError::Cancelled("operator interrupt".to_string()));
    }
    if deliver_messages(runner, ctx, emitter).await {
        return Ok(WaitStepResult::EventDelivered);
    }
    wait_for_message(runner).await?;
    if deliver_messages(runner, ctx, emitter).await {
        return Ok(WaitStepResult::EventDelivered);
    }
    Ok(WaitStepResult::ContinueWaiting)
}

/// Before a turn ends: delivers waiting messages, and while the agent's background jobs run, waits
/// for their notices. Returns whether anything arrived, so the agent continues instead of finishing.
async fn wait_for_background_jobs(
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<bool, AppError> {
    let owner = runner.store.owner();
    let jobs = crate::tools::jobs::JobManager::global();
    if !jobs.has_running(owner) {
        return Ok(deliver_messages(runner, ctx, emitter).await);
    }

    emitter.wait_for_display_drain().await?;
    runner.sink.resume_spinner_with_message("waiting...");
    while jobs.has_running(owner) {
        if let WaitStepResult::EventDelivered = wait_step(runner, ctx, emitter).await? {
            runner.sink.resume_spinner_with_message("thinking...");
            return Ok(true);
        }
    }

    runner.sink.resume_spinner_with_message("thinking...");
    Ok(deliver_messages(runner, ctx, emitter).await)
}

struct RunnerStepContext<'a> {
    current_messages: &'a mut Vec<ChatMessage>,
    new_turn_messages: &'a mut Vec<ChatMessage>,
    repeat_tracker: &'a mut RepeatedCallTracker,
    request_count: &'a mut usize,
    all_completion_calls: &'a mut Vec<CompletionCall>,
    total_usage: &'a mut StructuralUsage,
}

enum StepExecutionResult {
    Complete(PromptResponse),
    Continue,
    EmptyCompletion,
}

fn tool_result_contents(message: ChatMessage) -> Vec<rho_harness_core::model::UserContent> {
    match message {
        ChatMessage::User { content } => content,
        ChatMessage::Assistant { .. } | ChatMessage::System { .. } => Vec::new(),
    }
}

/// Runs the batch and adds its results to the turn; returns whether a tool ended the turn.
async fn dispatch_tool_calls(
    outcome_calls: &[ToolCall],
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<bool, AppError> {
    let mut exec_ctx = ToolExecutionContext {
        tools: &runner.tools,
        sink: &runner.sink,
        repeat_tracker: ctx.repeat_tracker,
        lifecycle: runner.lifecycle.as_ref(),
        permission: runner.permission.as_ref(),
        on_denied: runner.on_denied.as_ref(),
    };

    emitter.wait_for_display_drain().await?;
    let outcomes = if batch_is_concurrency_safe(&runner.tools, outcome_calls) {
        execute_concurrent_tool_calls(outcome_calls, &mut exec_ctx).await
    } else {
        let mut sequential = Vec::with_capacity(outcome_calls.len());
        for call in outcome_calls {
            emitter.ensure_live()?;
            let outcome = execute_single_tool_call(call, &mut exec_ctx).await;
            let stopped = matches!(outcome.1, CallControl::Stop(_));
            sequential.push(outcome);
            if stopped {
                break;
            }
        }
        sequential
    };

    let mut results = Vec::with_capacity(outcomes.len());
    let mut ends_turn = false;
    for (res_msg, control) in outcomes {
        results.extend(tool_result_contents(res_msg));
        emitter.send(MultiTurnStreamItem::ToolExecutionCommitted).await?;
        match control {
            CallControl::Stop(reason) => return Err(AppError::Cancelled(reason)),
            CallControl::EndTurn => ends_turn = true,
            CallControl::Continue => {}
        }
    }
    let batch = ChatMessage::User { content: results };
    ctx.current_messages.push(batch.clone());
    ctx.new_turn_messages.push(batch);
    Ok(ends_turn)
}

fn build_assistant_message(outcome: &TurnStreamOutcome) -> ChatMessage {
    let mut content = outcome.content.clone();
    let has_reasoning_blocks = content.iter().any(|c| matches!(c, AssistantContent::Reasoning(_)));
    if !has_reasoning_blocks && !outcome.reasoning.is_empty() {
        content.insert(0, AssistantContent::Reasoning(Reasoning::new(&outcome.reasoning)));
    }
    ChatMessage::Assistant { id: None, content }
}

fn completion_request(
    runner: &AgentRunner,
    messages: &[ChatMessage],
    tools_def: &[ModelToolDefinition],
) -> ModelCompletionRequest {
    ModelCompletionRequest {
        preamble: (!runner.preamble.is_empty()).then(|| runner.preamble.clone()),
        prompt: String::new(),
        messages: messages.to_vec(),
        tools: tools_def.to_vec(),
        max_tokens: None,
        temperature: None,
    }
}

fn tool_definitions(tools: &[Arc<dyn EngineTool>]) -> Vec<ModelToolDefinition> {
    tools
        .iter()
        .map(|t| ModelToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters(),
        })
        .collect()
}

async fn execute_agent_step(
    runner: &AgentRunner,
    turn: usize,
    tools_def: &[ModelToolDefinition],
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<StepExecutionResult, AppError> {
    emitter.ensure_live()?;
    let req = completion_request(runner, ctx.current_messages, tools_def);
    *ctx.request_count += 1;

    let outcome = call_model(runner, &req, emitter).await?;
    *ctx.total_usage = ctx.total_usage.merge(&outcome.usage);

    let completion_call = CompletionCall {
        call_index: turn,
        usage: outcome.usage,
        finish_reason: outcome.finish_reason,
        provider_request_id: None,
    };
    ctx.all_completion_calls.push(completion_call.clone());
    emitter
        .send(MultiTurnStreamItem::CompletionCall(completion_call))
        .await?;

    let empty = outcome.content.is_empty() && outcome.reasoning.is_empty();
    if !empty {
        let asst = build_assistant_message(&outcome);
        ctx.current_messages.push(asst.clone());
        ctx.new_turn_messages.push(asst);
    }

    if outcome.tool_calls.is_empty() {
        let last_turn = turn >= runner.budget;
        if !last_turn && wait_for_background_jobs(runner, ctx, emitter).await? {
            return Ok(StepExecutionResult::Continue);
        }
        if empty {
            return Ok(StepExecutionResult::EmptyCompletion);
        }
        return Ok(StepExecutionResult::Complete(prompt_response(outcome.text, ctx)));
    }

    if dispatch_tool_calls(&outcome.tool_calls, runner, ctx, emitter).await? {
        return Ok(StepExecutionResult::Complete(prompt_response(outcome.text, ctx)));
    }
    deliver_messages(runner, ctx, emitter).await;
    Ok(StepExecutionResult::Continue)
}

fn prompt_response(output: String, ctx: &RunnerStepContext<'_>) -> PromptResponse {
    PromptResponse {
        output,
        usage: *ctx.total_usage,
        requests: *ctx.request_count,
        completion_calls: ctx.all_completion_calls.clone(),
    }
}

/// The error a turn ends with when the model answers with nothing, even after a nudge.
pub(crate) const EMPTY_RESPONSE_ERROR: &str = "model returned an empty response";

/// Anthropic's documented recovery for empty `end_turn` responses after tool results:
/// <https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons>
const EMPTY_COMPLETION_NUDGE: &str = "Please continue";

/// A reminder that the session still owns unmerged worktrees, or `None` once given or when there are none.
async fn worktree_reminder(runner: &AgentRunner, reminded: &mut bool) -> Option<ChatMessage> {
    if *reminded {
        return None;
    }
    let store = runner.worktrees.as_ref()?;
    let pending = store.owned(&runner.working_dir, runner.store.owner()).await;
    if pending.is_empty() {
        return None;
    }
    *reminded = true;
    let listed: Vec<String> = pending.iter().map(|w| format!("`{}` ({})", w.id, w.label)).collect();
    Some(ChatMessage::user(format!(
        "[Before finishing: this session still owns unmerged agent worktrees: {}. You are responsible for them: review each with the worktree tool, then merge or discard it, or tell the user why it is being kept. Unmerged worktrees are deleted 24 hours after they were created.]",
        listed.join(", ")
    )))
}

async fn initial_turn_messages(runner: &AgentRunner) -> Result<(Vec<ChatMessage>, Vec<ChatMessage>), AppError> {
    let mut current_messages = if runner.history.is_empty() {
        runner.store.active_messages().await?
    } else {
        runner.history.clone()
    };
    let mut new_turn_messages = Vec::new();
    if !runner.prompt.trim().is_empty() {
        let user_msg = ChatMessage::user(runner.prompt.trim());
        current_messages.push(user_msg.clone());
        new_turn_messages.push(user_msg);
    }
    Ok((current_messages, new_turn_messages))
}

async fn run_agent_loop(runner: AgentRunner, emitter: &TurnEmitter) -> Result<(), AppError> {
    let mut saved = TurnCheckpoint {
        exists: runner.store.has_checkpoint().await?,
        len: 0,
    };
    let (mut current_messages, mut new_turn_messages) = initial_turn_messages(&runner).await?;

    let mut turn = 0;
    let mut total_usage = StructuralUsage::default();
    let mut repeat_tracker = runner.repeat_tracker.clone();
    let mut request_count = 0;
    let mut all_completion_calls = Vec::new();
    let mut nudged = false;
    let mut reminded = false;

    let tools_def = tool_definitions(&runner.tools);

    while turn < runner.budget {
        turn += 1;
        let mut step_ctx = RunnerStepContext {
            current_messages: &mut current_messages,
            new_turn_messages: &mut new_turn_messages,
            repeat_tracker: &mut repeat_tracker,
            request_count: &mut request_count,
            all_completion_calls: &mut all_completion_calls,
            total_usage: &mut total_usage,
        };
        let step = match execute_agent_step(&runner, turn, &tools_def, &mut step_ctx, emitter).await {
            Ok(step) => step,
            Err(error) => {
                return Err(keep_partial_turn(runner.store.as_ref(), &mut saved, &new_turn_messages, error).await);
            }
        };
        match step {
            StepExecutionResult::Complete(resp) => {
                if let Some(reminder) = worktree_reminder(&runner, &mut reminded).await {
                    current_messages.push(reminder.clone());
                    new_turn_messages.push(reminder);
                    continue;
                }
                emitter.ensure_live()?;
                runner
                    .store
                    .commit(saved.unsaved(&new_turn_messages), saved.exists)
                    .await?;
                return emitter.send(MultiTurnStreamItem::FinalResponse(resp)).await;
            }
            StepExecutionResult::Continue => {
                nudged = false;
                saved.persist(runner.store.as_ref(), &new_turn_messages).await?;
                if runner.store.keeps_tool_events() {
                    runner.sink.record_tool_events().await?;
                }
            }
            StepExecutionResult::EmptyCompletion if !nudged => {
                nudged = true;
                let nudge = ChatMessage::user(EMPTY_COMPLETION_NUDGE);
                current_messages.push(nudge.clone());
                new_turn_messages.push(nudge);
            }
            StepExecutionResult::EmptyCompletion => {
                saved.persist(runner.store.as_ref(), &new_turn_messages).await?;
                return Err(AppError::Provider(EMPTY_RESPONSE_ERROR.to_string()));
            }
        }
    }

    saved.persist(runner.store.as_ref(), &new_turn_messages).await?;
    Err(AppError::ModelBudgetExhausted {
        max_turns: runner.budget,
    })
}

struct TurnCheckpoint {
    exists: bool,
    len: usize,
}

impl TurnCheckpoint {
    fn unsaved(&self, new_turn_messages: &[ChatMessage]) -> Vec<ChatMessage> {
        new_turn_messages[self.len..].to_vec()
    }

    async fn persist(
        &mut self,
        store: &dyn super::store::TurnStore,
        new_turn_messages: &[ChatMessage],
    ) -> Result<(), AppError> {
        let complete = crate::engine::runner::history::without_unanswered_tool_calls(new_turn_messages);
        if complete.len() <= self.len {
            return Ok(());
        }
        store.extend_checkpoint(complete[self.len..].to_vec()).await?;
        self.exists = true;
        self.len = complete.len();
        Ok(())
    }
}

async fn keep_partial_turn(
    store: &dyn super::store::TurnStore,
    saved: &mut TurnCheckpoint,
    new_turn_messages: &[ChatMessage],
    error: AppError,
) -> AppError {
    if !new_turn_messages
        .iter()
        .any(|m| matches!(m, ChatMessage::Assistant { .. }))
    {
        return error;
    }
    match saved.persist(store, new_turn_messages).await {
        Ok(()) => error,
        Err(save_error) => AppError::Session(format!(
            "{error}; the turn's completed work could not be kept: {save_error}"
        )),
    }
}

#[cfg(test)]
mod tests;
