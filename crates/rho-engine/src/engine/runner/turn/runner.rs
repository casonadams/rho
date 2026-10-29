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

struct ReceiverStream<T>(tokio::sync::mpsc::Receiver<T>);

impl<T> Stream for ReceiverStream<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
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
    quality_gate_enabled: bool,
    max_quality_gate_retries: usize,
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
            quality_gate_enabled: true,
            max_quality_gate_retries: 3,
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

    pub fn quality_gate(mut self, enabled: bool) -> Self {
        self.quality_gate_enabled = enabled;
        self
    }

    pub fn max_quality_gate_retries(mut self, max: usize) -> Self {
        self.max_quality_gate_retries = max;
        self
    }

    pub fn stream(self) -> Pin<Box<dyn Stream<Item = Result<MultiTurnStreamItem, AppError>> + Send>> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            run_agent_loop(self, tx).await;
        });

        Box::pin(ReceiverStream(rx))
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
    tx: &tokio::sync::mpsc::Sender<Result<MultiTurnStreamItem, AppError>>,
) -> bool {
    match event {
        ModelStreamEvent::Text(t) => {
            outcome.text.push_str(&t);
            tx.send(Ok(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Text(t))))
                .await
                .is_ok()
        }
        ModelStreamEvent::Reasoning(r) => {
            outcome.reasoning.push_str(&r);
            tx.send(Ok(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(r))))
                .await
                .is_ok()
        }
        ModelStreamEvent::ToolCall(tc) => {
            let args_str = serde_json::to_string(&tc.function.arguments).unwrap_or_default();
            let _ = tx
                .send(Ok(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Arguments {
                    id: tc.id.clone(),
                    json: args_str,
                })))
                .await;
            let ok = tx
                .send(Ok(MultiTurnStreamItem::ToolCall { tool_call: tc.clone() }))
                .await
                .is_ok();
            outcome.tool_calls.push(tc);
            ok
        }
        ModelStreamEvent::Usage(u) => {
            outcome.usage = outcome.usage.merge(&u);
            true
        }
        ModelStreamEvent::Finish(f) => {
            outcome.finish_reason = Some(f);
            true
        }
    }
}

async fn stream_turn(
    mut stream: Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>,
    tx: &tokio::sync::mpsc::Sender<Result<MultiTurnStreamItem, AppError>>,
) -> Result<TurnStreamOutcome, AppError> {
    let mut outcome = TurnStreamOutcome::default();
    while let Some(res) = stream.next().await {
        match res {
            Ok(event) => {
                if !stream_single_event(event, &mut outcome, tx).await {
                    return Ok(outcome);
                }
            }
            Err(e) => return Err(e),
        }
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
) -> Result<Option<String>, AppError> {
    tokio::select! {
        res = notif_rx.recv() => {
            match res {
                Ok(notif) => Ok(Some(notif)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Ok(None),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => Ok(None),
            }
        }
        _ = async {
            match cancellation {
                Some(c) => c.wait_cancelled().await,
                None => std::future::pending::<()>().await,
            }
        } => Err(AppError::Cancelled("operator interrupt".to_string())),
        _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => Ok(None),
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

    if let Some(notif) = wait_for_background_events(notif_rx, runner.cancellation.as_ref()).await? {
        let msg = ChatMessage::user(notif);
        ctx.current_messages.push(msg.clone());
        ctx.new_turn_messages.push(msg);
        return Ok(WaitStepResult::EventDelivered);
    }

    Ok(WaitStepResult::ContinueWaiting)
}

async fn wait_for_background_jobs(runner: &AgentRunner, ctx: &mut RunnerStepContext<'_>) -> Result<bool, AppError> {
    if !crate::tools::jobs::JobManager::global().has_running() {
        return Ok(false);
    }

    runner.sink.resume_spinner_with_message("waiting for background job...");
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

async fn commit_turn_messages(session_manager: &SessionManager, new_messages: Vec<ChatMessage>, has_checkpoint: bool) {
    if new_messages.is_empty() {
        return;
    }
    if has_checkpoint {
        let _ = session_manager.promote_checkpoint(new_messages).await;
    } else {
        let _ = session_manager
            .append_messages(&session_manager.session_id, new_messages)
            .await;
    }
}

struct RunnerStepContext<'a> {
    current_messages: &'a mut Vec<ChatMessage>,
    new_turn_messages: &'a mut Vec<ChatMessage>,
    repeat_tracker: &'a mut RepeatedCallTracker,
    all_requests: &'a mut Vec<ModelCompletionRequest>,
    all_completion_calls: &'a mut Vec<CompletionCall>,
    total_usage: &'a mut StructuralUsage,
    quality_gate_retries: &'a mut usize,
}

enum StepExecutionResult {
    Complete(PromptResponse),
    Continue,
}

async fn dispatch_tool_calls(
    outcome_calls: &[ToolCall],
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    tx: &tokio::sync::mpsc::Sender<Result<MultiTurnStreamItem, AppError>>,
) -> Result<(), AppError> {
    let mut exec_ctx = ToolExecutionContext {
        tools: &runner.tools,
        sink: &runner.sink,
        repeat_tracker: ctx.repeat_tracker,
        lifecycle: runner.lifecycle.as_ref(),
        permission: runner.permission.as_ref(),
    };

    for call in outcome_calls {
        let (res_msg, stop_reason) = execute_single_tool_call(call, &mut exec_ctx).await;
        ctx.current_messages.push(res_msg.clone());
        ctx.new_turn_messages.push(res_msg);
        let _ = tx.send(Ok(MultiTurnStreamItem::ToolExecutionCommitted)).await;
        if let Some(reason) = stop_reason {
            return Err(AppError::Cancelled(reason));
        }
    }
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

async fn evaluate_quality_gate(runner: &AgentRunner, ctx: &mut RunnerStepContext<'_>) -> bool {
    if !runner.quality_gate_enabled || *ctx.quality_gate_retries >= runner.max_quality_gate_retries {
        return false;
    }
    let working_dir = runner.working_dir.clone();
    runner
        .sink
        .resume_spinner_with_message("verifying code quality with ripwire...");
    let gate_res = tokio::task::spawn_blocking(move || {
        crate::engine::runner::turn::quality::check_completion_quality(&working_dir)
    })
    .await
    .unwrap_or(None);
    runner.sink.resume_spinner_with_message("thinking...");

    if let Some(feedback) = gate_res {
        let attempt = *ctx.quality_gate_retries + 1;
        let max_attempts = runner.max_quality_gate_retries;
        *ctx.quality_gate_retries += 1;
        let content = format!("{feedback}\n\n[Quality Gate Attempt {attempt} of {max_attempts}]");
        let feedback_msg = ChatMessage::user(content);
        ctx.current_messages.push(feedback_msg.clone());
        ctx.new_turn_messages.push(feedback_msg);
        return true;
    }
    false
}

async fn execute_agent_step(
    runner: &AgentRunner,
    turn: usize,
    tools_def: &[ModelToolDefinition],
    ctx: &mut RunnerStepContext<'_>,
    tx: &tokio::sync::mpsc::Sender<Result<MultiTurnStreamItem, AppError>>,
) -> Result<StepExecutionResult, AppError> {
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
    ctx.all_requests.push(req.clone());

    let stream = runner.model.stream(req).await?;
    let outcome = stream_turn(stream, tx).await?;
    *ctx.total_usage = ctx.total_usage.merge(&outcome.usage);

    let completion_call = CompletionCall {
        call_index: turn,
        usage: outcome.usage,
        finish_reason: outcome.finish_reason,
        provider_request_id: None,
    };
    ctx.all_completion_calls.push(completion_call.clone());
    let _ = tx.send(Ok(MultiTurnStreamItem::CompletionCall(completion_call))).await;

    let asst = build_assistant_message(&outcome);
    ctx.current_messages.push(asst.clone());
    ctx.new_turn_messages.push(asst);

    if outcome.tool_calls.is_empty() {
        if wait_for_background_jobs(runner, ctx).await? {
            return Ok(StepExecutionResult::Continue);
        }
        if evaluate_quality_gate(runner, ctx).await {
            return Ok(StepExecutionResult::Continue);
        }
        let resp = PromptResponse {
            output: outcome.text,
            usage: *ctx.total_usage,
            requests: ctx.all_requests.clone(),
            completion_calls: ctx.all_completion_calls.clone(),
        };
        return Ok(StepExecutionResult::Complete(resp));
    }

    dispatch_tool_calls(&outcome.tool_calls, runner, ctx, tx).await?;
    check_steering(runner.steering.as_ref(), ctx.current_messages, ctx.new_turn_messages).await;
    Ok(StepExecutionResult::Continue)
}

async fn run_agent_loop(runner: AgentRunner, tx: tokio::sync::mpsc::Sender<Result<MultiTurnStreamItem, AppError>>) {
    let has_checkpoint = runner.session_manager.load_checkpoint().await.unwrap_or(None).is_some();
    let initial_visible_history = if runner.history.is_empty() {
        runner.session_manager.active_messages().await.unwrap_or_default()
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
    let mut all_requests = Vec::new();
    let mut all_completion_calls = Vec::new();
    let mut quality_gate_retries = 0;

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
            all_requests: &mut all_requests,
            all_completion_calls: &mut all_completion_calls,
            total_usage: &mut total_usage,
            quality_gate_retries: &mut quality_gate_retries,
        };
        match execute_agent_step(&runner, turn, &tools_def, &mut step_ctx, &tx).await {
            Ok(StepExecutionResult::Complete(resp)) => {
                commit_turn_messages(&runner.session_manager, new_turn_messages, has_checkpoint).await;
                let _ = tx.send(Ok(MultiTurnStreamItem::FinalResponse(resp))).await;
                return;
            }
            Ok(StepExecutionResult::Continue) => {}
            Err(e) => {
                let _ = tx.send(Err(e)).await;
                return;
            }
        }
    }

    let _ = runner.session_manager.save_checkpoint(new_turn_messages).await;
    let _ = tx
        .send(Err(AppError::ModelBudgetExhausted {
            max_turns: runner.budget,
        }))
        .await;
}

#[cfg(test)]
#[derive(Default)]
struct TestStepBuffers {
    current_messages: Vec<ChatMessage>,
    new_turn_messages: Vec<ChatMessage>,
    all_requests: Vec<ModelCompletionRequest>,
    all_completion_calls: Vec<CompletionCall>,
    total_usage: StructuralUsage,
    quality_gate_retries: usize,
}

#[cfg(test)]
impl<'a> RunnerStepContext<'a> {
    fn from_test_buffers(buf: &'a mut TestStepBuffers, tracker: &'a mut RepeatedCallTracker) -> Self {
        Self {
            current_messages: &mut buf.current_messages,
            new_turn_messages: &mut buf.new_turn_messages,
            repeat_tracker: tracker,
            all_requests: &mut buf.all_requests,
            all_completion_calls: &mut buf.all_completion_calls,
            total_usage: &mut buf.total_usage,
            quality_gate_retries: &mut buf.quality_gate_retries,
        }
    }
}

#[cfg(test)]
fn test_agent_env() -> (AgentRunner, tempfile::TempDir, TestStepBuffers, RepeatedCallTracker) {
    let temp = tempfile::tempdir().unwrap();
    let sm = rho_harness_core::session::SessionManager::new(temp.path(), None).unwrap();
    let cfg = crate::engine::runner::sink::TerminalSinkConfig {
        model_label: "test".into(),
        run_tracker: Default::default(),
    };
    let pres: Arc<dyn rho_harness_core::presentation::Presenter> =
        Arc::new(crate::engine::eval::presenter::NoopPresenter);
    let sink = TerminalApprovalSink::new(&pres, cfg, sm.clone());
    let mock = Arc::new(crate::provider::adapter::MockAdapter::new("m", "p"));
    let runner = AgentRunner::new(mock, Vec::new(), sm, sink);
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

    #[tokio::test]
    async fn test_wait_for_background_jobs_flow() {
        let (runner, _temp, mut buf, mut tracker) = test_agent_env();
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
    async fn test_evaluate_quality_gate_scenarios() {
        let (runner, temp, mut buf, mut tracker) = test_agent_env();
        let mut ctx = RunnerStepContext::from_test_buffers(&mut buf, &mut tracker);

        let runner_off = runner.clone().quality_gate(false);
        assert!(!evaluate_quality_gate(&runner_off, &mut ctx).await);

        let runner_retries = runner.clone().max_quality_gate_retries(1);
        *ctx.quality_gate_retries = 1;
        assert!(!evaluate_quality_gate(&runner_retries, &mut ctx).await);

        *ctx.quality_gate_retries = 0;
        let runner_clean = runner.working_dir(temp.path()).quality_gate(true);
        assert!(!evaluate_quality_gate(&runner_clean, &mut ctx).await);
    }
}
