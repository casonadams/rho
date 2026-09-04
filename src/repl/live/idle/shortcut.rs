use std::time::{Duration, Instant};

use super::super::batch::LiveBatch;
use super::super::modal::{open_model_selector, open_session_selector, open_tree_selector};
use super::super::navigation::{
    ModelCycleContext, copy_last_message, cycle_model, cycle_thinking_level, paste_clipboard, update_footer,
};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::repl::ReplSession;
use crate::ui::interactive::{InputAction, TerminalBackend, TerminalController};

pub(super) struct IdleShortcutContext<'a, 'b, 'c, B: TerminalBackend> {
    pub controller: &'a mut TerminalController<B>,
    pub session: &'b mut ReplSession,
    pub engine: &'c mut AgentEngine,
    pub last_escape_time: &'a mut Option<Instant>,
}

pub(super) async fn handle_shortcut_action<B: TerminalBackend>(
    action: InputAction,
    ctx: IdleShortcutContext<'_, '_, '_, B>,
    batch: &mut LiveBatch,
) -> Result<()> {
    match action {
        InputAction::Cancel => {
            let was_empty = ctx.controller.state().editor().text().is_empty();
            ctx.controller.state_mut().autocomplete.close();
            ctx.controller.state_mut().editor_mut().set_text("");
            if was_empty {
                let now = Instant::now();
                if let Some(prev) = ctx.last_escape_time.take() {
                    if now.duration_since(prev) < Duration::from_millis(500)
                        && let Ok(tree) = ctx.engine.session_manager.load_tree().await
                    {
                        open_tree_selector(&tree, ctx.controller);
                    } else {
                        *ctx.last_escape_time = Some(now);
                    }
                } else {
                    *ctx.last_escape_time = Some(now);
                }
            } else {
                *ctx.last_escape_time = None;
            }
            ctx.controller.redraw()?;
        }
        InputAction::ToggleExpandTools => {
            let expanded = ctx.controller.toggle_tools_expanded()?;
            ctx.session.renderer.print_status(&format!(
                "Tool output: {}",
                if expanded { "expanded" } else { "collapsed" }
            ));
        }
        InputAction::ModelSelect => {
            open_model_selector(ctx.session, ctx.controller);
            ctx.controller.redraw()?;
        }
        InputAction::ModelCycleForward => {
            let mut cycle_ctx = ModelCycleContext {
                session: ctx.session,
                engine: ctx.engine,
                controller: ctx.controller,
            };
            cycle_model(&mut cycle_ctx, 1).await;
            batch.flush(ctx.controller, true)?;
        }
        InputAction::ModelCycleBackward => {
            let mut cycle_ctx = ModelCycleContext {
                session: ctx.session,
                engine: ctx.engine,
                controller: ctx.controller,
            };
            cycle_model(&mut cycle_ctx, -1).await;
            batch.flush(ctx.controller, true)?;
        }
        InputAction::ThinkingCycle => {
            cycle_thinking_level(ctx.session, ctx.engine, ctx.controller).await;
            batch.flush(ctx.controller, true)?;
        }
        InputAction::ThinkingToggle => {
            let hide = ctx.controller.toggle_thinking()?;
            ctx.session
                .renderer
                .print_status(&format!("Thinking blocks: {}", if hide { "hidden" } else { "visible" }));
        }
        InputAction::MessageCopy => {
            copy_last_message(ctx.session, ctx.controller);
            batch.flush(ctx.controller, true)?;
        }
        InputAction::ClipboardPasteImage => {
            paste_clipboard(&ctx.session.renderer, ctx.controller);
            batch.flush(ctx.controller, true)?;
        }
        InputAction::SessionTree => {
            if let Ok(tree) = ctx.engine.session_manager.load_tree().await {
                open_tree_selector(&tree, ctx.controller);
                ctx.controller.redraw()?;
            }
        }
        InputAction::SessionResume => {
            open_session_selector(&ctx.session.config.sessions_dir, ctx.controller);
            ctx.controller.redraw()?;
        }
        InputAction::SessionNew => {
            *ctx.engine =
                crate::platform::agent_engine(ctx.session.config.clone(), ctx.session.auth_store.clone(), None).await?;
            ctx.controller.clear_transcript();
            ctx.session.renderer.print_status("Context cleared");
            update_footer(ctx.controller.state_mut(), ctx.session, ctx.engine);
            ctx.controller.redraw()?;
        }
        InputAction::Suspend => {
            crate::platform::suspend::suspend_process();
            ctx.controller.redraw()?;
        }
        _ => {}
    }
    Ok(())
}
