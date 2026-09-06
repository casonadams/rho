use crossterm::event::KeyEvent;

use super::super::batch::LiveBatch;
use super::super::navigation::{apply_completion, navigate_history_next, navigate_history_previous, paste_clipboard};
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{InputAction, QueueKind, TerminalBackend, TerminalController, UiEffect, map_key};

pub(super) enum TurnKeyResult {
    Handled,
    Cancelled,
    Ignored,
}

pub(super) struct TurnInputContext<'a, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub history: &'a mut InteractiveHistory,
    pub completions: &'a CompletionSet,
    pub batch: &'a mut LiveBatch,
    pub steering: &'a crate::repl::coordinator::SharedSteeringQueue,
    pub session: &'a mut crate::repl::ReplSession,
    pub model_switch: &'a std::sync::Arc<rho_engine::engine::runner::SharedModelSwitch>,
    pub shared_auth: Option<std::sync::Arc<tokio::sync::Mutex<crate::auth::AuthStore>>>,
}

async fn open_model_selector_and_flush<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<()> {
    super::super::modal::open_model_selector(ctx.session, ctx.controller);
    ctx.batch.flush(ctx.controller, true)
}

async fn flush_if<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>, changed: bool) -> Result<()> {
    if changed {
        ctx.batch.flush(ctx.controller, true)?;
    }
    Ok(())
}

fn queue_status_message<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>, kind: QueueKind, text: &str) {
    if kind == QueueKind::Steering && !crate::repl::commands::is_slash_command(text) {
        ctx.steering.enqueue(text.to_string());
        ctx.controller.set_system_message("[Steering queued for tool boundary]");
    } else if kind == QueueKind::FollowUp {
        ctx.controller
            .set_system_message("[Follow-up queued for turn completion]");
    }
}

async fn handle_edit_action<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    action: crate::ui::interactive::UiAction,
) -> Result<TurnKeyResult> {
    let effect = ctx.controller.state_mut().apply(action);
    if let UiEffect::Queued(ref msg) = effect {
        queue_status_message(ctx, msg.kind, &msg.text);
    }
    ctx.batch.flush(ctx.controller, true)?;
    Ok(TurnKeyResult::Handled)
}

fn handle_display_toggle<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>, action: &InputAction) {
    if *action == InputAction::ToggleExpandTools {
        let expanded = ctx.controller.toggle_tools_expanded().unwrap_or(false);
        let state = if expanded { "expanded" } else { "collapsed" };
        ctx.session.renderer.print_status(&format!("Tool output: {state}"));
    } else {
        let hide = ctx.controller.toggle_thinking().unwrap_or(false);
        let state = if hide { "hidden" } else { "visible" };
        ctx.session.renderer.print_status(&format!("Thinking blocks: {state}"));
    }
}

fn handle_dequeue<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<TurnKeyResult> {
    let queued = ctx.controller.state_mut().dequeue_all();
    if !queued.is_empty() {
        let text = queued.into_iter().map(|m| m.text).collect::<Vec<_>>().join("\n");
        ctx.controller.state_mut().editor_mut().set_text(&text);
        ctx.batch.flush(ctx.controller, true)?;
    }
    Ok(TurnKeyResult::Handled)
}

fn handle_clear<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>) -> Result<TurnKeyResult> {
    ctx.controller.state_mut().autocomplete.close();
    ctx.controller.state_mut().editor_mut().set_text("");
    ctx.batch.flush(ctx.controller, true)?;
    Ok(TurnKeyResult::Handled)
}

async fn handle_turn_action<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    action: InputAction,
) -> Result<TurnKeyResult> {
    if model_action(ctx, &action).await? {
        return Ok(TurnKeyResult::Handled);
    }
    if view_action(ctx, &action).await? {
        return Ok(TurnKeyResult::Handled);
    }
    if let InputAction::Edit(edit) = action {
        return handle_edit_action(ctx, edit).await;
    }
    history_or_completion(ctx, &action).await
}

async fn model_action<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>, action: &InputAction) -> Result<bool> {
    match action {
        InputAction::ModelSelect => open_model_selector_and_flush(ctx).await?,
        InputAction::ModelCycleForward => super::model_switch::cycle_turn_model(ctx, 1).await?,
        InputAction::ModelCycleBackward => super::model_switch::cycle_turn_model(ctx, -1).await?,
        InputAction::ThinkingCycle => super::model_switch::cycle_turn_thinking(ctx).await?,
        _ => return Ok(false),
    }
    Ok(true)
}

async fn view_action<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>, action: &InputAction) -> Result<bool> {
    match action {
        InputAction::ToggleExpandTools | InputAction::ThinkingToggle => {
            handle_display_toggle(ctx, action);
        }
        InputAction::ClipboardPasteImage => {
            paste_clipboard(&ctx.session.renderer, ctx.controller);
            ctx.batch.flush(ctx.controller, true)?;
        }
        InputAction::DequeueQueued => {
            handle_dequeue(ctx)?;
        }
        InputAction::Clear => {
            handle_clear(ctx)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn history_or_completion<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    action: &InputAction,
) -> Result<TurnKeyResult> {
    let moved = match action {
        InputAction::HistoryPrevious => navigate_history_previous(ctx.controller, ctx.history),
        InputAction::HistoryNext => navigate_history_next(ctx.controller, ctx.history),
        InputAction::Complete => apply_completion(ctx.controller, ctx.completions),
        InputAction::Cancel => return Ok(TurnKeyResult::Cancelled),
        _ => return Ok(TurnKeyResult::Ignored),
    };
    flush_if(ctx, moved).await?;
    Ok(TurnKeyResult::Handled)
}

pub(super) async fn handle_turn_key<B: TerminalBackend>(
    key: KeyEvent,
    ctx: &mut TurnInputContext<'_, B>,
) -> Result<TurnKeyResult> {
    let action = map_key(key);
    handle_turn_action(ctx, action).await
}

pub(super) fn reconcile_consumed_steering<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    steering: &crate::repl::coordinator::SharedSteeringQueue,
) -> bool {
    let consumed = steering.take_consumed();
    if consumed.is_empty() {
        return false;
    }
    controller
        .state_mut()
        .retain_queued(|msg| msg.kind != QueueKind::Steering || !consumed.contains(&msg.text));
    true
}
