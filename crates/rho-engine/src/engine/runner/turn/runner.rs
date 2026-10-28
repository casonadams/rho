use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::types::{CompletionCall, PromptResponse, StructuralUsage};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::runner::turn::types::SteeringQueueProvider;
use crate::hook::{LifecycleHook, ToolDispatchAction};
use crate::provider::adapter::{FinishReason, ModelCompletionRequest, ModelStreamEvent, ModelToolDefinition};
use crate::repeat::RepeatedCallTracker;
use crate::tools::EngineTool;
use futures::Stream;
use futures::StreamExt;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolCall};
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
    stream_port: Option<ToolStreamPort>,
    steering: Option<Arc<dyn SteeringQueueProvider>>,
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
            repeat_tracker: RepeatedCallTracker::new(working_dir),
            lifecycle: None,
            stream_port: None,
            steering: None,
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

    pub fn stream_port(mut self, stream_port: ToolStreamPort) -> Self {
        self.stream_port = Some(stream_port);
        self
    }

    pub fn steering(mut self, steering: Option<Arc<dyn SteeringQueueProvider>>) -> Self {
        self.steering = steering;
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
        ModelStreamEvent::Reasoning(r) => tx
            .send(Ok(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(r))))
            .await
            .is_ok(),
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
}

async fn execute_single_tool_call(call: &ToolCall, ctx: &mut ToolExecutionContext<'_>) -> ChatMessage {
    if let Err(msg) = ctx.repeat_tracker.check(&call.function.name, &call.function.arguments) {
        return ChatMessage::User {
            content: vec![rho_harness_core::model::UserContent::ToolResult(
                rho_harness_core::model::ToolResult::new(call.id.clone(), call.function.name.clone(), msg),
            )],
        };
    }

    if let Some(lifecycle) = ctx.lifecycle
        && let ToolDispatchAction::Deny(reason) = lifecycle
            .on_tool_dispatch(&call.function.name, &call.function.arguments)
            .await
    {
        return ChatMessage::User {
            content: vec![rho_harness_core::model::UserContent::ToolResult(
                rho_harness_core::model::ToolResult::new(
                    call.id.clone(),
                    call.function.name.clone(),
                    format!("Denied by hook: {reason}"),
                ),
            )],
        };
    }

    ctx.sink.tool_start(&call.function.name, &call.function.arguments);
    let tool_opt = ctx.tools.iter().find(|t| t.name() == call.function.name);
    let (output, is_error) = match tool_opt {
        Some(t) => match t.execute(call.function.arguments.clone()).await {
            Ok(r) => (r.content, r.is_error),
            Err(e) => (format!("Error: {e}"), true),
        },
        None => (format!("Error: Tool '{}' not found", call.function.name), true),
    };

    ctx.sink.tool_finished(crate::engine::runner::sink::ToolFinishDetails {
        name: &call.function.name,
        arguments: &call.function.arguments,
        output: &output,
        is_error,
    });

    if let Some(lifecycle) = ctx.lifecycle {
        let _ = lifecycle
            .on_tool_outcome(&call.function.name, &call.function.arguments, &output, is_error)
            .await;
    }

    ChatMessage::User {
        content: vec![rho_harness_core::model::UserContent::ToolResult(
            rho_harness_core::model::ToolResult::new(call.id.clone(), call.function.name.clone(), output),
        )],
    }
}

async fn check_steering(
    steering: Option<&Arc<dyn SteeringQueueProvider>>,
    current_messages: &mut Vec<ChatMessage>,
    new_turn_messages: &mut Vec<ChatMessage>,
) {
    if let Some(provider) = steering {
        for text in provider.poll_steering().await {
            let steer_msg = ChatMessage::user(text);
            current_messages.push(steer_msg.clone());
            new_turn_messages.push(steer_msg);
        }
    }
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
}

enum StepExecutionResult {
    Complete(PromptResponse),
    Continue,
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

    if outcome.tool_calls.is_empty() {
        let asst = ChatMessage::assistant(outcome.text.clone());
        ctx.current_messages.push(asst.clone());
        ctx.new_turn_messages.push(asst);
        let resp = PromptResponse {
            output: outcome.text,
            usage: *ctx.total_usage,
            requests: ctx.all_requests.clone(),
            completion_calls: ctx.all_completion_calls.clone(),
        };
        return Ok(StepExecutionResult::Complete(resp));
    }

    let mut asst_content = Vec::new();
    if !outcome.text.is_empty() {
        asst_content.push(AssistantContent::text(outcome.text.clone()));
    }
    for call in outcome.tool_calls.clone() {
        asst_content.push(AssistantContent::ToolCall(call));
    }
    let asst = ChatMessage::Assistant {
        id: None,
        content: asst_content,
    };
    ctx.current_messages.push(asst.clone());
    ctx.new_turn_messages.push(asst);

    let mut exec_ctx = ToolExecutionContext {
        tools: &runner.tools,
        sink: &runner.sink,
        repeat_tracker: ctx.repeat_tracker,
        lifecycle: runner.lifecycle.as_ref(),
    };

    for call in &outcome.tool_calls {
        let res_msg = execute_single_tool_call(call, &mut exec_ctx).await;
        ctx.current_messages.push(res_msg.clone());
        ctx.new_turn_messages.push(res_msg);
        let _ = tx.send(Ok(MultiTurnStreamItem::ToolExecutionCommitted)).await;
    }

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
