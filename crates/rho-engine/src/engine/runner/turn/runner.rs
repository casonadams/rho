use super::checkpoint::{TurnCheckpoint, keep_partial_turn};
use crate::engine::compactor::llm::ModelHandle;
use crate::engine::metrics::types::{CompletionCall, PromptResponse, StructuralUsage};
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::engine::runner::turn::types::{CancellationSignal, SteeringQueueProvider, unless_cancelled};
use crate::hook::LifecycleHook;
use crate::provider::adapter::{ModelCompletionRequest, ModelToolDefinition};
use crate::repeat::RepeatedCallTracker;
use crate::tools::EngineTool;
use crate::tools::engine_tool::TurnToolContext;
use futures::Stream;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ChatMessage, Reasoning, ToolCall};
use rho_harness_core::presentation::ToolStreamPort;
use rho_harness_core::session::SessionManager;
use rho_harness_core::thinking::ThinkingLevel;
use std::pin::Pin;
use std::sync::Arc;

mod builder;
mod delivery;
mod dispatch;
mod model_call;

use delivery::wait_for_background_jobs;
#[cfg(test)]
use delivery::{WaitStepResult, deliver_messages, wait_step};
pub use dispatch::ToolObserver;
use dispatch::{
    CallControl, ToolExecutionContext, batch_is_concurrency_safe, execute_concurrent_tool_calls,
    execute_single_tool_call,
};
#[cfg(test)]
use dispatch::{DispatchDecision, evaluate_tool_dispatch};
#[cfg(test)]
use model_call::{MODEL_CALL_ATTEMPTS, retry_delay};
use model_call::{TurnStreamOutcome, call_model};

#[derive(Clone, Debug)]
pub enum StreamEvent {
    Text(String),
    Reasoning(String),
    /// A fragment of tool call `id`'s argument JSON, empty when the call only begins or ends.
    Arguments {
        id: String,
        name: String,
        json: String,
    },
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

/// The error a turn ends with when the operator interrupts it.
fn interrupted() -> AppError {
    AppError::Cancelled("operator interrupt".to_string())
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
            return Err(interrupted());
        }
        Ok(())
    }

    async fn send(&self, item: MultiTurnStreamItem) -> Result<(), AppError> {
        self.ensure_live()?;
        self.tx.send(Ok(item)).await.map_err(|_| interrupted())
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
    steering: Option<Arc<dyn SteeringQueueProvider>>,
    inbox: Option<crate::mailbox::Mailbox>,
    cancellation: Option<CancellationSignal>,
    tool_observer: Option<Arc<dyn ToolObserver>>,
    working_dir: std::path::PathBuf,
    monitor: Option<rho_harness_core::agents::AgentMonitor>,
    project: Option<Arc<crate::engine::context::ProjectContext>>,
    worktrees: Option<crate::worktree::WorktreeStore>,
    context_budget: Option<crate::engine::tracking::ContextBudget>,
    a2ui_widgets: bool,
    a2ui_config: rho_harness_core::config::A2uiConfig,
    thinking_level: Option<ThinkingLevel>,
}

impl AgentRunner {
    /// Runs `fut` unless the turn is interrupted first.
    async fn unless_cancelled<T>(&self, fut: impl std::future::Future<Output = T>) -> Result<T, AppError> {
        unless_cancelled(self.cancellation.as_ref(), fut)
            .await
            .ok_or_else(interrupted)
    }

    fn bind_tools(&mut self, history: &[ChatMessage]) {
        let turn_ctx = TurnToolContext {
            permission: self.permission.clone(),
            lifecycle: self.lifecycle.clone(),
            cancellation: self.cancellation.clone(),
            progress: ToolStreamPort::new(Some(self.sink.clone())),
            parent_history: history.into(),
            monitor: self.monitor.clone(),
            project: self.project.clone(),
            session: self.session_manager.clone(),
            inbox: self.inbox.clone(),
            steering: self.steering.clone(),
            presenter: self.lifecycle.as_ref().map(|h| Arc::clone(h.presenter())),
            a2ui_widgets: self.a2ui_widgets,
            a2ui_config: self.a2ui_config.clone(),
        };
        self.tools = self
            .tools
            .iter()
            .map(|tool| tool.bind_turn(&turn_ctx).unwrap_or_else(|| tool.clone()))
            .collect();
    }

    pub fn stream(self) -> Pin<Box<dyn Stream<Item = Result<MultiTurnStreamItem, AppError>> + Send>> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let emitter = TurnEmitter {
            tx,
            cancellation: self.cancellation.clone(),
        };
        let task = tokio::spawn(async move {
            let _answering = self.inbox.as_ref().map(crate::mailbox::Mailbox::answering);
            if let Err(error) = run_agent_loop(self, &emitter).await {
                let _ = emitter.tx.send(Err(error)).await;
            }
        });

        Box::pin(TurnStream { rx, task })
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

/// Appends `notice` to the last tool result in `results`, the newest content of the next request;
/// false when there is none.
fn append_to_last_tool_result(results: &mut [rho_harness_core::model::UserContent], notice: &str) -> bool {
    use rho_harness_core::model::{ToolResultContent, UserContent};
    let Some(UserContent::ToolResult(result)) = results
        .iter_mut()
        .rev()
        .find(|item| matches!(item, UserContent::ToolResult(_)))
    else {
        return false;
    };
    match result.content.last_mut() {
        Some(ToolResultContent::Text(text)) => text.text.push_str(&format!("\n\n{notice}")),
        _ => result.content.push(ToolResultContent::text(notice)),
    }
    true
}

/// Runs the batch and adds its results to the turn; returns whether a tool ended the turn.
async fn dispatch_tool_calls(
    outcome: &TurnStreamOutcome,
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<bool, AppError> {
    let outcome_calls = &outcome.tool_calls;
    let mut exec_ctx = ToolExecutionContext {
        tools: &runner.tools,
        sink: &runner.sink,
        repeat_tracker: ctx.repeat_tracker,
        lifecycle: runner.lifecycle.as_ref(),
        permission: runner.permission.as_ref(),
        observer: runner.tool_observer.as_ref(),
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
    let used = crate::engine::display::consumed_context_tokens(&outcome.usage);
    if let Some(budget) = &runner.context_budget {
        budget.notice(used, |notice| append_to_last_tool_result(&mut results, notice));
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
        thinking_level: runner.thinking_level,
    }
}

pub(in crate::engine) fn tool_definitions(tools: &[Arc<dyn EngineTool>]) -> Vec<ModelToolDefinition> {
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
    *ctx.request_count += 1;
    let messages: &[ChatMessage] = ctx.current_messages;
    let outcome = call_model(runner, || completion_request(runner, messages, tools_def), emitter).await?;
    *ctx.total_usage = ctx.total_usage.merge(&outcome.usage);

    let completion_call = CompletionCall {
        call_index: turn,
        usage: outcome.usage,
        finish_reason: outcome.finish_reason,
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

    if dispatch_tool_calls(&outcome, runner, ctx, emitter).await? {
        return Ok(StepExecutionResult::Complete(prompt_response(outcome.text, ctx)));
    }
    delivery::deliver_messages(runner, ctx, emitter).await?;
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

/// Stops the turn, its steps saved, once the next request's prompt would pass where compaction
/// starts. No request follows the turn's last one, so that one never stops.
fn stop_if_compaction_due(
    runner: &AgentRunner,
    turn: usize,
    calls: &[CompletionCall],
    appended: &[ChatMessage],
) -> Result<(), AppError> {
    let (Some(budget), Some(call)) = (&runner.context_budget, calls.last()) else {
        return Ok(());
    };
    if turn >= runner.budget {
        return Ok(());
    }
    let used = next_prompt_tokens(call, appended, runner.model.model_name());
    if budget.compaction_due(used) {
        return Err(AppError::CompactionDue { used });
    }
    Ok(())
}

/// The prompt of the request after `call`: its prompt and reply, plus the estimated size of
/// everything else the step `appended` after the reply (tool results, delivered messages).
fn next_prompt_tokens(call: &CompletionCall, appended: &[ChatMessage], model: &str) -> u64 {
    let appended_inputs: usize = appended
        .iter()
        .filter(|message| !matches!(message, ChatMessage::Assistant { .. }))
        .map(|message| rho_harness_core::tokens::estimate_message_tokens(message, model))
        .sum();
    crate::engine::display::consumed_context_tokens(&call.usage) + call.usage.output_tokens + appended_inputs as u64
}

async fn run_agent_loop(mut runner: AgentRunner, emitter: &TurnEmitter) -> Result<(), AppError> {
    let mut saved = TurnCheckpoint {
        exists: runner.store.has_checkpoint().await?,
        len: 0,
    };
    let (mut current_messages, mut new_turn_messages) = initial_turn_messages(&runner).await?;
    runner.bind_tools(&current_messages);

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
        let sent = current_messages.len();
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
                stop_if_compaction_due(&runner, turn, &all_completion_calls, &current_messages[sent..])?;
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

#[cfg(test)]
mod tests;
