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

pub(crate) use dispatch::{DispatchDecision, apply_tool_outcome_hooks, evaluate_tool_dispatch};
use dispatch::{
    ToolExecutionContext, batch_is_concurrency_safe, execute_concurrent_tool_calls, execute_single_tool_call,
};

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Text(String),
    Reasoning(String),
    Arguments { id: String, json: String },
}

#[derive(Clone, Debug)]
pub enum MultiTurnStreamItem {
    StreamAssistantItem(StreamEvent),
    ToolCall { tool_call: ToolCall },
    ToolExecutionCommitted,
    ModelTurnRetried(ModelRetry),
    CompletionCall(CompletionCall),
    FinalResponse(PromptResponse),
    DisplayBarrier(Arc<tokio::sync::Notify>),
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
    session_manager: SessionManager,
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
    cancellation: Option<CancellationSignal>,
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
        Self {
            model,
            tools,
            session_manager,
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
            cancellation: None,
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
            session: Some(self.session_manager.clone()),
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

async fn check_steering(
    steering: Option<&Arc<dyn SteeringQueueProvider>>,
    current_messages: &mut Vec<ChatMessage>,
    new_turn_messages: &mut Vec<ChatMessage>,
) {
    let notifs = crate::engine::runner::turn::types::drain_background_notifications();
    append_notification_messages(notifs, current_messages, new_turn_messages);
    if let Some(provider) = steering {
        let steered = provider.poll_steering().await;
        append_notification_messages(steered, current_messages, new_turn_messages);
    }
}

fn drain_background_to_context(ctx: &mut RunnerStepContext<'_>) -> bool {
    let notifs = crate::engine::runner::turn::types::drain_background_notifications();
    if notifs.is_empty() {
        return false;
    }
    append_notification_messages(notifs, ctx.current_messages, ctx.new_turn_messages);
    true
}

async fn drain_steering_to_context(
    steering: Option<&Arc<dyn SteeringQueueProvider>>,
    ctx: &mut RunnerStepContext<'_>,
) -> bool {
    let Some(provider) = steering else {
        return false;
    };
    let steered = provider.poll_steering().await;
    if steered.is_empty() {
        return false;
    }
    append_notification_messages(steered, ctx.current_messages, ctx.new_turn_messages);
    true
}

async fn wait_for_background_events(
    notif_rx: &mut tokio::sync::broadcast::Receiver<String>,
    cancellation: Option<&CancellationSignal>,
) -> Result<(), AppError> {
    tokio::select! {
        _ = notif_rx.recv() => Ok(()),
        _ = async {
            match cancellation {
                Some(c) => c.wait_cancelled().await,
                None => std::future::pending::<()>().await,
            }
        } => Err(AppError::Cancelled("operator interrupt".to_string())),
        _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => Ok(()),
    }
}

enum WaitStepResult {
    EventDelivered,
    ContinueWaiting,
}

async fn wait_step(
    runner: &AgentRunner,
    notif_rx: &mut tokio::sync::broadcast::Receiver<String>,
    ctx: &mut RunnerStepContext<'_>,
) -> Result<WaitStepResult, AppError> {
    if runner.cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
        return Err(AppError::Cancelled("operator interrupt".to_string()));
    }

    if drain_background_to_context(ctx) || drain_steering_to_context(runner.steering.as_ref(), ctx).await {
        return Ok(WaitStepResult::EventDelivered);
    }

    wait_for_background_events(notif_rx, runner.cancellation.as_ref()).await?;
    if drain_background_to_context(ctx) {
        return Ok(WaitStepResult::EventDelivered);
    }
    Ok(WaitStepResult::ContinueWaiting)
}

async fn wait_for_background_jobs(
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<bool, AppError> {
    if !crate::tools::jobs::JobManager::global().has_running() {
        return Ok(false);
    }

    emitter.wait_for_display_drain().await?;
    runner.sink.resume_spinner_with_message("waiting...");
    let mut notif_rx = crate::engine::runner::turn::types::subscribe_background_notifications();

    while crate::tools::jobs::JobManager::global().has_running() {
        if let WaitStepResult::EventDelivered = wait_step(runner, &mut notif_rx, ctx).await? {
            runner.sink.resume_spinner_with_message("thinking...");
            return Ok(true);
        }
    }

    runner.sink.resume_spinner_with_message("thinking...");
    Ok(drain_background_to_context(ctx))
}

async fn commit_turn_messages(
    session_manager: &SessionManager,
    new_messages: Vec<ChatMessage>,
    has_checkpoint: bool,
) -> Result<(), AppError> {
    if new_messages.is_empty() {
        return Ok(());
    }
    if has_checkpoint {
        session_manager.promote_checkpoint(new_messages).await
    } else {
        session_manager
            .append_messages(&session_manager.session_id, new_messages)
            .await
    }
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

async fn dispatch_tool_calls(
    outcome_calls: &[ToolCall],
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<(), AppError> {
    let mut exec_ctx = ToolExecutionContext {
        tools: &runner.tools,
        sink: &runner.sink,
        repeat_tracker: ctx.repeat_tracker,
        lifecycle: runner.lifecycle.as_ref(),
        permission: runner.permission.as_ref(),
    };

    emitter.wait_for_display_drain().await?;
    let outcomes = if batch_is_concurrency_safe(&runner.tools, outcome_calls) {
        execute_concurrent_tool_calls(outcome_calls, &mut exec_ctx).await
    } else {
        let mut sequential = Vec::with_capacity(outcome_calls.len());
        for call in outcome_calls {
            emitter.ensure_live()?;
            let outcome = execute_single_tool_call(call, &mut exec_ctx).await;
            let stopped = outcome.1.is_some();
            sequential.push(outcome);
            if stopped {
                break;
            }
        }
        sequential
    };

    let mut results = Vec::with_capacity(outcomes.len());
    for (res_msg, stop_reason) in outcomes {
        results.extend(tool_result_contents(res_msg));
        emitter.send(MultiTurnStreamItem::ToolExecutionCommitted).await?;
        if let Some(reason) = stop_reason {
            return Err(AppError::Cancelled(reason));
        }
    }
    let batch = ChatMessage::User { content: results };
    ctx.current_messages.push(batch.clone());
    ctx.new_turn_messages.push(batch);
    Ok(())
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
        if wait_for_background_jobs(runner, ctx, emitter).await? {
            return Ok(StepExecutionResult::Continue);
        }
        if empty {
            return Ok(StepExecutionResult::EmptyCompletion);
        }
        let resp = PromptResponse {
            output: outcome.text,
            usage: *ctx.total_usage,
            requests: *ctx.request_count,
            completion_calls: ctx.all_completion_calls.clone(),
        };
        return Ok(StepExecutionResult::Complete(resp));
    }

    dispatch_tool_calls(&outcome.tool_calls, runner, ctx, emitter).await?;
    check_steering(runner.steering.as_ref(), ctx.current_messages, ctx.new_turn_messages).await;
    Ok(StepExecutionResult::Continue)
}

/// Anthropic's documented recovery for empty `end_turn` responses after tool results:
/// <https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons>
const EMPTY_COMPLETION_NUDGE: &str = "Please continue";

/// A reminder that the session still owns unmerged worktrees, or `None` once given or when there are none.
async fn worktree_reminder(runner: &AgentRunner, reminded: &mut bool) -> Option<ChatMessage> {
    if *reminded {
        return None;
    }
    let store = runner.worktrees.as_ref()?;
    let pending = store
        .owned(&runner.working_dir, &runner.session_manager.session_id)
        .await;
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
        runner.session_manager.active_messages().await?
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
        exists: runner.session_manager.load_checkpoint().await?.is_some(),
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
                return Err(keep_partial_turn(&runner.session_manager, &mut saved, &new_turn_messages, error).await);
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
                commit_turn_messages(&runner.session_manager, saved.unsaved(&new_turn_messages), saved.exists).await?;
                return emitter.send(MultiTurnStreamItem::FinalResponse(resp)).await;
            }
            StepExecutionResult::Continue => {
                nudged = false;
                saved.persist(&runner.session_manager, &new_turn_messages).await?;
                runner.sink.record_tool_events().await?;
            }
            StepExecutionResult::EmptyCompletion if !nudged => {
                nudged = true;
                let nudge = ChatMessage::user(EMPTY_COMPLETION_NUDGE);
                current_messages.push(nudge.clone());
                new_turn_messages.push(nudge);
            }
            StepExecutionResult::EmptyCompletion => {
                saved.persist(&runner.session_manager, &new_turn_messages).await?;
                return Err(AppError::Provider("model returned an empty response".to_string()));
            }
        }
    }

    saved.persist(&runner.session_manager, &new_turn_messages).await?;
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
        session_manager: &SessionManager,
        new_turn_messages: &[ChatMessage],
    ) -> Result<(), AppError> {
        let complete = crate::engine::runner::history::without_unanswered_tool_calls(new_turn_messages);
        if complete.len() <= self.len {
            return Ok(());
        }
        session_manager.extend_checkpoint(complete[self.len..].to_vec()).await?;
        self.exists = true;
        self.len = complete.len();
        Ok(())
    }
}

async fn keep_partial_turn(
    session_manager: &SessionManager,
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
    match saved.persist(session_manager, new_turn_messages).await {
        Ok(()) => error,
        Err(save_error) => AppError::Session(format!(
            "{error}; the turn's completed work could not be kept: {save_error}"
        )),
    }
}

#[cfg(test)]
mod tests;
