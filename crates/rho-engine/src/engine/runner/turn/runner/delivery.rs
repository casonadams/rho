use super::{AgentRunner, MultiTurnStreamItem, RunnerStepContext, TurnEmitter, interrupted};
use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;

/// Adds the agent's waiting messages to the turn: its mailbox (background notices, messages from
/// other agents and the user) and the user's queued steering. Returns whether any arrived.
pub(super) async fn deliver_messages(
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<bool, AppError> {
    let mut messages: Vec<String> = Vec::new();
    if let Some(inbox) = &runner.inbox {
        messages.extend(inbox.for_model(&inbox.take()));
    }
    if let Some(provider) = &runner.steering {
        messages.extend(provider.poll_steering().await);
    }
    if messages.is_empty() {
        return Ok(false);
    }
    for message in &messages {
        let message = ChatMessage::user(message.clone());
        ctx.current_messages.push(message.clone());
        ctx.new_turn_messages.push(message);
    }
    emitter.send(MultiTurnStreamItem::MessagesDelivered(messages)).await?;
    Ok(true)
}

/// Waits for a message, a cancellation, or a short tick (a job can end without sending anything).
async fn wait_for_message(runner: &AgentRunner) -> Result<(), AppError> {
    runner
        .unless_cancelled(crate::mailbox::wait_for_message_or_tick(runner.inbox.as_ref()))
        .await
}

pub(super) enum WaitStepResult {
    EventDelivered,
    ContinueWaiting,
}

pub(super) async fn wait_step(
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<WaitStepResult, AppError> {
    if runner.cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
        return Err(interrupted());
    }
    if deliver_messages(runner, ctx, emitter).await? {
        return Ok(WaitStepResult::EventDelivered);
    }
    wait_for_message(runner).await?;
    if deliver_messages(runner, ctx, emitter).await? {
        return Ok(WaitStepResult::EventDelivered);
    }
    Ok(WaitStepResult::ContinueWaiting)
}

/// Before a turn ends: delivers waiting messages, and while the agent's background jobs run, waits
/// for their notices. Returns whether anything arrived, so the agent continues instead of finishing.
pub(super) async fn wait_for_background_jobs(
    runner: &AgentRunner,
    ctx: &mut RunnerStepContext<'_>,
    emitter: &TurnEmitter,
) -> Result<bool, AppError> {
    let owner = runner.store.owner();
    let jobs = crate::tools::jobs::JobManager::global();
    if !jobs.has_running(owner) {
        return deliver_messages(runner, ctx, emitter).await;
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
    deliver_messages(runner, ctx, emitter).await
}
