use std::time::{Duration, Instant};

use super::super::batch::LiveBatch;
use super::super::modal::{open_model_selector, open_tree_selector};
use super::super::navigation::{
    ModelCycleContext, copy_last_message, cycle_model, cycle_thinking_level, paste_clipboard_async,
};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{InputAction, TerminalBackend, TerminalController};

pub(crate) struct IdleShortcutContext<'a, 'b, 'c, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub session: &'b mut ReplSession,
    pub engine: &'c mut AgentEngine,
    pub last_escape_time: &'a mut Option<Instant>,
}

async fn cycle_model_by(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    direction: i32,
    batch: &mut LiveBatch,
) -> Result<()> {
    let mut cycle_ctx = ModelCycleContext {
        session: ctx.session,
        engine: ctx.engine,
        controller: ctx.controller,
    };
    cycle_model(&mut cycle_ctx, direction).await;
    batch.flush(ctx.controller, true)
}

fn clear_input(ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>) -> Result<()> {
    ctx.controller.state_mut().autocomplete.close();
    ctx.controller.state_mut().editor_mut().set_text("");
    ctx.controller.redraw()?;
    Ok(())
}

async fn maybe_open_tree_on_double_escape(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    now: Instant,
) -> Result<()> {
    match ctx.last_escape_time.take() {
        Some(prev) if now.duration_since(prev) < Duration::from_millis(500) => {
            if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                open_tree_selector(&tree, ctx.controller);
            }
            Ok(())
        }
        _ => {
            *ctx.last_escape_time = Some(now);
            Ok(())
        }
    }
}

async fn handle_clear_or_cancel(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    is_cancel: bool,
) -> Result<()> {
    let was_empty = ctx.controller.state().editor().text().is_empty();
    clear_input(ctx)?;
    if is_cancel {
        if was_empty {
            maybe_open_tree_on_double_escape(ctx, Instant::now()).await?;
        } else {
            *ctx.last_escape_time = None;
        }
    }
    Ok(())
}

fn toggle_tools(controller: &mut TerminalController<impl TerminalBackend>) -> Result<()> {
    let expanded = !controller.tools_expanded();
    let state = if expanded { "expanded" } else { "collapsed" };
    controller.set_system_message(format!("Tool output: {state}"));
    controller.set_tools_expanded(expanded)?;
    Ok(())
}

fn toggle_thinking(controller: &mut TerminalController<impl TerminalBackend>) -> Result<()> {
    let hidden = !controller.hide_thinking();
    let state = if hidden { "hidden" } else { "visible" };
    controller.set_system_message(format!("Thinking blocks: {state}"));
    controller.set_hide_thinking(hidden)?;
    Ok(())
}

fn flush_after(ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>, batch: &mut LiveBatch) -> Result<()> {
    batch.flush(ctx.controller, true)
}

async fn handle_model_cycle_action(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    action: &InputAction,
    batch: &mut LiveBatch,
) -> Result<bool> {
    match action {
        InputAction::ModelCycleForward => cycle_model_by(ctx, 1, batch).await?,
        InputAction::ModelCycleBackward => cycle_model_by(ctx, -1, batch).await?,
        _ => return Ok(false),
    }
    Ok(true)
}

async fn handle_auxiliary_action(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    action: &InputAction,
    batch: &mut LiveBatch,
) -> Result<bool> {
    match action {
        InputAction::ThinkingCycle => {
            cycle_thinking_level(ctx.session, ctx.engine, ctx.controller).await;
            flush_after(ctx, batch)?;
        }
        InputAction::MessageCopy => {
            copy_last_message(ctx.session, ctx.controller);
            flush_after(ctx, batch)?;
        }
        InputAction::ClipboardPasteImage => {
            paste_clipboard_async(&ctx.session.renderer, ctx.controller).await;
            flush_after(ctx, batch)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn handle_ui_toggle_or_selector(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    action: &InputAction,
) -> Result<bool> {
    match action {
        InputAction::ToggleExpandTools => toggle_tools(ctx.controller)?,
        InputAction::ThinkingToggle => toggle_thinking(ctx.controller)?,
        InputAction::ModelSelect => {
            open_model_selector(ctx.session, ctx.controller);
            ctx.controller.redraw()?;
        }
        InputAction::Suspend => {
            crate::platform::suspend::suspend_process();
            ctx.controller.redraw()?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

async fn dispatch_shortcut(
    ctx: &mut IdleShortcutContext<'_, '_, '_, impl TerminalBackend>,
    action: &InputAction,
    batch: &mut LiveBatch,
) -> Result<()> {
    match action {
        InputAction::Clear => handle_clear_or_cancel(ctx, false).await?,
        InputAction::Cancel => handle_clear_or_cancel(ctx, true).await?,
        other => {
            if !handle_ui_toggle_or_selector(ctx, other).await? && !handle_model_cycle_action(ctx, other, batch).await?
            {
                handle_auxiliary_action(ctx, other, batch).await?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn handle_shortcut_action<B: TerminalBackend>(
    action: InputAction,
    mut ctx: IdleShortcutContext<'_, '_, '_, B>,
    batch: &mut LiveBatch,
) -> Result<()> {
    dispatch_shortcut(&mut ctx, &action, batch).await?;
    Ok(())
}
