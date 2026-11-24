use crossterm::event::KeyEvent;

use super::super::batch::LiveBatch;
use super::super::navigation::{
    apply_completion, navigate_history_next, navigate_history_previous, paste_clipboard_async, record_submission,
};
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
    pub engine: Option<&'a crate::engine::AgentEngine>,
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
    if crate::repl::commands::is_slash_command(text) {
        ctx.controller
            .set_system_message("[Command queued for turn completion]");
    } else if kind == QueueKind::Steering {
        ctx.steering.enqueue(text.to_string());
        ctx.controller.set_system_message("[Steering queued for tool boundary]");
    } else if kind == QueueKind::FollowUp {
        ctx.controller
            .set_system_message("[Follow-up queued for turn completion]");
    }
}

async fn apply_mid_turn_command<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    result: crate::repl::commands::CommandResult,
) -> Result<()> {
    use crate::repl::commands::CommandResult;
    match result {
        CommandResult::OpenSettingsSelector
        | CommandResult::OpenModelSelector
        | CommandResult::OpenModelsSelector
        | CommandResult::OpenToolsSelector
        | CommandResult::OpenHelpSelector => {
            crate::repl::live::message::open_selector_modal(ctx.session, ctx.controller, &result);
        }
        CommandResult::OpenAgentsSelector => {
            if let Some(engine) = ctx.engine {
                crate::repl::live::modal::agents::open_agents_selector(ctx.controller, engine.agent_monitor())?;
            }
        }
        CommandResult::ModelChanged {
            new_model,
            new_provider,
        } => {
            let provider = new_provider.unwrap_or_else(|| ctx.session.config.provider.clone());
            super::model_switch::apply_turn_model_switch(super::model_switch::TurnModelSwitchInput {
                model: &new_model,
                provider: &provider,
                save_as_default: false,
                config: &mut ctx.session.config,
                auth_store: &ctx.session.auth_store,
                renderer: &ctx.session.renderer,
                controller: ctx.controller,
                batch: ctx.batch,
            })
            .await?;
        }
        CommandResult::ThinkingChanged { level } => {
            ctx.session.config.thinking_level = level;
            if let Some(engine) = ctx.engine {
                crate::repl::live::navigation::update_footer(ctx.controller.state_mut(), ctx.session, engine);
            }
        }
        _ => {}
    }
    Ok(())
}

async fn execute_mid_turn_command<B: TerminalBackend>(ctx: &mut TurnInputContext<'_, B>, text: &str) -> Result<()> {
    let mut cmd_ctx = crate::repl::commands::SlashCommandContext {
        config: &mut ctx.session.config,
        auth_store: &mut ctx.session.auth_store,
        renderer: &ctx.session.renderer,
        session_id: ctx.engine.map(|e| e.session_manager.session_id.as_str()),
        session_manager: ctx.engine.map(|e| &e.session_manager),
        engine: ctx.engine,
        home_dir: None,
    };
    if let Some(result) = crate::repl::commands::SlashCommandHandler::handle(text, &mut cmd_ctx).await? {
        apply_mid_turn_command(ctx, result).await?;
    }
    Ok(())
}

async fn handle_edit_action<B: TerminalBackend>(
    ctx: &mut TurnInputContext<'_, B>,
    action: crate::ui::interactive::UiAction,
) -> Result<TurnKeyResult> {
    let effect = ctx.controller.state_mut().apply(action);
    crate::repl::live::autocomplete::update_autocomplete_state(ctx.controller, ctx.completions);
    if let UiEffect::Queued(ref msg) = effect {
        record_submission(ctx.history, &msg.text)?;
        if crate::repl::commands::mid_turn_policy(&msg.text) == crate::repl::commands::MidTurnPolicy::Immediate {
            ctx.controller.state_mut().pop_back_queued();
            execute_mid_turn_command(ctx, &msg.text).await?;
        } else {
            queue_status_message(ctx, msg.kind, &msg.text);
        }
    }
    ctx.batch.flush(ctx.controller, true)?;
    Ok(TurnKeyResult::Handled)
}

fn toggle_tools_expanded<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let expanded = !controller.tools_expanded();
    let state = if expanded { "expanded" } else { "collapsed" };
    controller.set_system_message(format!("Tool output: {state}"));
    let _ = controller.set_tools_expanded(expanded);
}

fn toggle_hide_thinking<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let hide = !controller.hide_thinking();
    let state = if hide { "hidden" } else { "visible" };
    controller.set_system_message(format!("Thinking blocks: {state}"));
    let _ = controller.set_hide_thinking(hide);
}

pub(super) fn handle_display_toggle<B: TerminalBackend>(controller: &mut TerminalController<B>, action: &InputAction) {
    match action {
        InputAction::ToggleExpandTools => toggle_tools_expanded(controller),
        InputAction::ThinkingToggle => toggle_hide_thinking(controller),
        _ => {}
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
            handle_display_toggle(ctx.controller, action);
        }
        InputAction::ClipboardPasteImage => {
            paste_clipboard_async(&ctx.session.renderer, ctx.controller).await;
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
    for text in consumed {
        let _ = controller.push_transcript_item(crate::ui::interactive::TranscriptItem::UserMessage(text));
    }
    true
}
