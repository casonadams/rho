use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::types::{CompletionCall, PromptResponse, StructuralUsage};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::runner::turn::types::{CancellationSignal, SteeringQueueProvider};
use crate::hook::{LifecycleHook, ToolDispatchAction};
use crate::provider::adapter::{FinishReason, ModelCompletionRequest, ModelStreamEvent, ModelToolDefinition};
use crate::repeat::RepeatedCallTracker;
use crate::tools::EngineTool;
use futures::Stream;
use futures::StreamExt;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ChatMessage, Reasoning, ToolCall};
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::session::SessionManager;
use std::pin::Pin;
use std::sync::Arc;

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
    ModelTurnRetried,
    CompletionCall(CompletionCall),
    FinalResponse(PromptResponse),
}

type TurnItem = Result<MultiTurnStreamItem, AppError>;

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

    pub fn working_dir(mut self, working_dir: impl Into<std::path::PathBuf>) -> Self {
        self.working_dir = working_dir.into();
        self
    }

    pub fn stream(self) -> Pin<Box<dyn Stream<Item = Result<MultiTurnStreamItem, AppError>> + Send>> {
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
    tool_calls: Vec<ToolCall>,
    usage: StructuralUsage,
    finish_reason: Option<FinishReason>,
}

async fn stream_single_event(
    event: ModelStreamEvent,
    outcome: &mut TurnStreamOutcome,
    emitter: &TurnEmitter,
) -> Result<(), AppError> {
    match event {
        ModelStreamEvent::Text(t) => {
            outcome.text.push_str(&t);
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
) -> Result<TurnStreamOutcome, AppError> {
    let mut outcome = TurnStreamOutcome::default();
    while let Some(res) = stream.next().await {
        stream_single_event(res?, &mut outcome, emitter).await?;
    }
    Ok(outcome)
}

struct ToolExecutionContext<'a> {
    tools: &'a [Arc<dyn EngineTool>],
    sink: &'a Arc<TerminalApprovalSink>,
    repeat_tracker: &'a mut RepeatedCallTracker,
    lifecycle: Option<&'a Arc<LifecycleHook>>,
    permission: Option<&'a Arc<crate::permission::PermissionHook>>,
}

enum DispatchDecision {
    Proceed(serde_json::Value),
    Deny(ChatMessage),
    Stop(ChatMessage, String),
}

async fn evaluate_tool_dispatch(
    call: &ToolCall,
    lifecycle: Option<&Arc<LifecycleHook>>,
    permission: Option<&Arc<crate::permission::PermissionHook>>,
) -> DispatchDecision {
    let mut args = call.function.arguments.clone();
    if let Some(lifecycle) = lifecycle {
        match lifecycle.on_tool_dispatch(&call.function.name, &args).await {
            ToolDispatchAction::Stop(reason) => {
                let msg = ChatMessage::User {
                    content: vec![rho_harness_core::model::UserContent::ToolResult(
                        rho_harness_core::model::ToolResult::new(
                            call.id.clone(),
                            call.function.name.clone(),
                            format!("Stopped by hook: {reason}"),
                        ),
                    )],
                };
                return DispatchDecision::Stop(msg, reason);
            }
            ToolDispatchAction::Deny(reason) => {
                let msg = ChatMessage::User {
                    content: vec![rho_harness_core::model::UserContent::ToolResult(
                        rho_harness_core::model::ToolResult::new(
                            call.id.clone(),
                            call.function.name.clone(),
                            format!("Denied by hook: {reason}"),
                        ),
                    )],
                };
                return DispatchDecision::Deny(msg);
            }
            ToolDispatchAction::RewriteArgs(new_args) => args = new_args,
            ToolDispatchAction::Proceed => {}
        }
    }

    if let Some(permission) = permission {
        match permission.check_permission(&call.function.name, &args).await {
            crate::permission::hook::PermissionAction::Deny(reason) => {
                let msg = ChatMessage::User {
                    content: vec![rho_harness_core::model::UserContent::ToolResult(
                        rho_harness_core::model::ToolResult::new(
                            call.id.clone(),
                            call.function.name.clone(),
                            format!("Permission denied: {reason}"),
                        ),
                    )],
                };
                return DispatchDecision::Deny(msg);
            }
            crate::permission::hook::PermissionAction::RewriteArgs(new_args) => args = new_args,
            crate::permission::hook::PermissionAction::Proceed => {}
        }
    }

    DispatchDecision::Proceed(args)
}

async fn apply_tool_outcome_hooks(
    lifecycle: Option<&Arc<LifecycleHook>>,
    tool_name: &str,
    args: &serde_json::Value,
    output: &mut String,
    is_error: bool,
) -> Option<String> {
    let lifecycle = lifecycle?;
    match lifecycle.on_tool_outcome(tool_name, args, output, is_error).await {
        crate::hook::ToolOutcomeAction::Proceed => None,
        crate::hook::ToolOutcomeAction::RewriteResult(new_output) => {
            *output = new_output;
            None
        }
        crate::hook::ToolOutcomeAction::Stop(reason) => Some(reason),
    }
}

async fn execute_single_tool_call(
    call: &ToolCall,
    ctx: &mut ToolExecutionContext<'_>,
) -> (ChatMessage, Option<String>) {
    if let Err(msg) = ctx.repeat_tracker.check(&call.function.name, &call.function.arguments) {
        return (
            ChatMessage::User {
                content: vec![rho_harness_core::model::UserContent::ToolResult(
                    rho_harness_core::model::ToolResult::new(call.id.clone(), call.function.name.clone(), msg),
                )],
            },
            None,
        );
    }

    let effective_args = match evaluate_tool_dispatch(call, ctx.lifecycle, ctx.permission).await {
        DispatchDecision::Proceed(args) => args,
        DispatchDecision::Deny(msg) => return (msg, None),
        DispatchDecision::Stop(msg, reason) => return (msg, Some(reason)),
    };

    ctx.sink.tool_start(&call.function.name, &effective_args);
    let tool_opt = ctx.tools.iter().find(|t| t.name() == call.function.name);
    let (mut output, is_error) = match tool_opt {
        Some(t) => match t.execute(effective_args.clone()).await {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (format!("Error: {e}"), true),
        },
        None => (format!("Error: Tool '{}' not found", call.function.name), true),
    };

    ctx.sink.tool_finished(crate::engine::runner::sink::ToolFinishDetails {
        name: &call.function.name,
        arguments: &effective_args,
        output: &output,
        is_error,
    });

    let stop_reason = apply_tool_outcome_hooks(
        ctx.lifecycle,
        &call.function.name,
        &effective_args,
        &mut output,
        is_error,
    )
    .await;

    (
        ChatMessage::User {
            content: vec![rho_harness_core::model::UserContent::ToolResult(
                rho_harness_core::model::ToolResult::new(call.id.clone(), call.function.name.clone(), output),
            )],
        },
        stop_reason,
    )
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

async fn wait_for_background_jobs(runner: &AgentRunner, ctx: &mut RunnerStepContext<'_>) -> Result<bool, AppError> {
    if !crate::tools::jobs::JobManager::global().has_running() {
        return Ok(false);
    }

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

    let mut results = Vec::with_capacity(outcome_calls.len());
    for call in outcome_calls {
        emitter.ensure_live()?;
        let (res_msg, stop_reason) = execute_single_tool_call(call, &mut exec_ctx).await;
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
    let mut content = Vec::new();
    if !outcome.reasoning.is_empty() {
        content.push(AssistantContent::Reasoning(Reasoning::new(&outcome.reasoning)));
    }
    if !outcome.text.is_empty() {
        content.push(AssistantContent::text(&outcome.text));
    }
    for call in &outcome.tool_calls {
        content.push(AssistantContent::ToolCall(call.clone()));
    }
    ChatMessage::Assistant { id: None, content }
}

async fn execute_agent_step(
    runner: &AgentRunner,
    turn: usize,
    tools_def: &[ModelToolDefinition],
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<StepExecutionResult, AppError> {
    emitter.ensure_live()?;
    let req = ModelCompletionRequest {
        preamble: if runner.preamble.is_empty() {
            None
        } else {
            Some(runner.preamble.clone())
        },
        prompt: String::new(),
        messages: ctx.current_messages.clone(),
        tools: tools_def.to_vec(),
        max_tokens: None,
        temperature: None,
    };
    *ctx.request_count += 1;

    let stream = runner.model.stream(req).await?;
    let outcome = stream_turn(stream, emitter).await?;
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

    let asst = build_assistant_message(&outcome);
    ctx.current_messages.push(asst.clone());
    ctx.new_turn_messages.push(asst);

    if outcome.tool_calls.is_empty() {
        if wait_for_background_jobs(runner, ctx).await? {
            return Ok(StepExecutionResult::Continue);
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

async fn run_agent_loop(runner: AgentRunner, emitter: &TurnEmitter) -> Result<(), AppError> {
    let pending = runner.session_manager.load_checkpoint().await?;
    let has_checkpoint = pending.is_some();
    let initial_visible_history = if runner.history.is_empty() {
        runner.session_manager.active_messages().await?
    } else {
        runner.history.clone()
    };
    let mut current_messages = initial_visible_history.clone();
    let mut new_turn_messages = Vec::new();

    if !runner.prompt.trim().is_empty() {
        let user_msg = ChatMessage::user(runner.prompt.trim());
        current_messages.push(user_msg.clone());
        new_turn_messages.push(user_msg);
    }

    let mut turn = 0;
    let mut total_usage = StructuralUsage::default();
    let mut repeat_tracker = runner.repeat_tracker.clone();
    let mut request_count = 0;
    let mut all_completion_calls = Vec::new();

    let tools_def: Vec<ModelToolDefinition> = runner
        .tools
        .iter()
        .map(|t| ModelToolDefinition {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters(),
        })
        .collect();

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
        if let StepExecutionResult::Complete(resp) =
            execute_agent_step(&runner, turn, &tools_def, &mut step_ctx, emitter).await?
        {
            emitter.ensure_live()?;
            commit_turn_messages(&runner.session_manager, new_turn_messages, has_checkpoint).await?;
            return emitter.send(MultiTurnStreamItem::FinalResponse(resp)).await;
        }
    }

    let mut checkpoint = pending.unwrap_or_default();
    checkpoint.extend(new_turn_messages);
    runner.session_manager.save_checkpoint(checkpoint).await?;
    Err(AppError::ModelBudgetExhausted {
        max_turns: runner.budget,
    })
}

#[cfg(test)]
#[derive(Default)]
struct TestStepBuffers {
    current_messages: Vec<ChatMessage>,
    new_turn_messages: Vec<ChatMessage>,
    request_count: usize,
    all_completion_calls: Vec<CompletionCall>,
    total_usage: StructuralUsage,
}

#[cfg(test)]
impl<'a> RunnerStepContext<'a> {
    fn from_test_buffers(buf: &'a mut TestStepBuffers, tracker: &'a mut RepeatedCallTracker) -> Self {
        Self {
            current_messages: &mut buf.current_messages,
            new_turn_messages: &mut buf.new_turn_messages,
            repeat_tracker: tracker,
            request_count: &mut buf.request_count,
            all_completion_calls: &mut buf.all_completion_calls,
            total_usage: &mut buf.total_usage,
        }
    }
}

#[cfg(test)]
fn test_agent_env(
    model: Arc<crate::provider::adapter::MockAdapter>,
) -> (AgentRunner, tempfile::TempDir, TestStepBuffers, RepeatedCallTracker) {
    let temp = tempfile::tempdir().unwrap();
    let sm = rho_harness_core::session::SessionManager::new(temp.path(), None).unwrap();
    let cfg = crate::engine::runner::sink::TerminalSinkConfig {
        model_label: "test".into(),
        run_tracker: Default::default(),
    };
    let pres: Arc<dyn rho_harness_core::presentation::Presenter> =
        Arc::new(crate::engine::eval::presenter::NoopPresenter);
    let sink = TerminalApprovalSink::new(&pres, cfg, sm.clone());
    let runner = AgentRunner::new(model, Vec::new(), sm, sink);
    (
        runner,
        temp,
        TestStepBuffers::default(),
        RepeatedCallTracker::new(std::path::PathBuf::from(".")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::MockAdapter;
    use rho_harness_core::model::ToolFunction;

    static BACKGROUND_QUEUE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn test_wait_for_background_jobs_flow() {
        let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
        let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
        let sig = CancellationSignal::default();
        let runner = runner.cancellation(Some(sig.clone()));
        let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);

        assert!(!wait_for_background_jobs(&runner, &mut ctx).await.unwrap());
        crate::tools::jobs::JobManager::global().register("bg-1".into(), "echo 1".into());
        tokio::spawn(async {
            crate::tools::jobs::JobManager::global().update_status(
                "bg-1",
                crate::tools::jobs::JobStatus::Completed {
                    exit_code: 0,
                    output: "ok".into(),
                },
            );
            crate::engine::runner::turn::types::enqueue_background_notification(
                "<system-notice>done</system-notice>".into(),
            );
        });
        assert!(wait_for_background_jobs(&runner, &mut ctx).await.unwrap());
        crate::tools::jobs::JobManager::global().register("bg-2".into(), "sleep".into());
        sig.cancel();
        assert!(matches!(
            wait_for_background_jobs(&runner, &mut ctx).await,
            Err(AppError::Cancelled(_))
        ));
        crate::tools::jobs::JobManager::global()
            .update_status("bg-2", crate::tools::jobs::JobStatus::Failed { error: "a".into() });
    }

    #[tokio::test]
    async fn agent_loop_stops_calling_model_once_receiver_is_dropped() {
        let model = Arc::new(MockAdapter::from_turns(vec![
            vec![ModelStreamEvent::ToolCall(ToolCall::new(
                "call-1",
                ToolFunction::new("read", serde_json::json!({"path": "a.txt"})),
            ))],
            vec![ModelStreamEvent::Text("unreachable".to_string())],
        ]));
        let (runner, _temp, _, _) = test_agent_env(model.clone());
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let emitter = TurnEmitter { tx, cancellation: None };
        let consumer = async move {
            let first = rx.recv().await;
            assert!(matches!(
                first,
                Some(Ok(MultiTurnStreamItem::StreamAssistantItem(
                    StreamEvent::Arguments { .. }
                )))
            ));
            drop(rx);
        };

        let (result, ()) = tokio::join!(run_agent_loop(runner.clone(), &emitter), consumer);

        assert!(matches!(result, Err(AppError::Cancelled(_))));
        assert_eq!(model.requests().len(), 1);
        assert!(runner.session_manager.active_messages().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn agent_loop_does_not_call_model_after_cancellation() {
        let model = Arc::new(MockAdapter::text("unreachable"));
        let (runner, _temp, _, _) = test_agent_env(model.clone());
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        let runner = runner.cancellation(Some(cancellation));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let emitter = TurnEmitter {
            tx,
            cancellation: runner.cancellation.clone(),
        };

        let result = run_agent_loop(runner, &emitter).await;

        assert!(matches!(result, Err(AppError::Cancelled(_))));
        assert!(model.requests().is_empty());
    }

    #[tokio::test]
    async fn agent_loop_reports_failed_commit_instead_of_completing() {
        let model = Arc::new(MockAdapter::text("answer with sk-live-secret-42"));
        let (runner, _temp, _, _) = test_agent_env(model);
        runner
            .session_manager
            .add_secrets_async(["sk-live-secret-42".to_string()])
            .await
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let emitter = TurnEmitter { tx, cancellation: None };

        let result = run_agent_loop(runner, &emitter).await;
        drop(emitter);

        assert!(result.is_err());
        while let Some(item) = rx.recv().await {
            assert!(!matches!(item, Ok(MultiTurnStreamItem::FinalResponse(_))));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn background_notification_is_injected_once_when_woken() {
        let _queue = BACKGROUND_QUEUE_TEST_LOCK.lock().await;
        let (runner, _temp, mut buf, mut tracker) = test_agent_env(Arc::new(MockAdapter::default()));
        let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);
        let mut notif_rx = crate::engine::runner::turn::types::subscribe_background_notifications();
        let enqueue = tokio::spawn(async {
            tokio::task::yield_now().await;
            crate::engine::runner::turn::types::enqueue_background_notification("bg-once-7d1".to_string());
        });

        let step = wait_step(&runner, &mut notif_rx, &mut ctx).await.unwrap();
        enqueue.await.unwrap();
        drain_background_to_context(&mut ctx);

        assert!(matches!(step, WaitStepResult::EventDelivered));
        assert_eq!(
            serde_json::to_string(&*ctx.current_messages)
                .unwrap()
                .matches("bg-once-7d1")
                .count(),
            1
        );
    }
}
