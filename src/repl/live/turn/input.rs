use crossterm::event::KeyEvent;

use super::super::batch::LiveBatch;
use super::super::navigation::{apply_completion, navigate_history_next, navigate_history_previous, paste_clipboard};
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::TerminalRenderer;
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
    pub renderer: &'a TerminalRenderer,
    pub batch: &'a mut LiveBatch,
    pub steering: &'a crate::repl::coordinator::SharedSteeringQueue,
}

pub(super) fn handle_turn_key<B: TerminalBackend>(
    key: KeyEvent,
    ctx: &mut TurnInputContext<'_, B>,
) -> Result<TurnKeyResult> {
    match map_key(key) {
        InputAction::Edit(action) => {
            let effect = ctx.controller.state_mut().apply(action);
            if let UiEffect::Queued(ref msg) = effect {
                if msg.kind == QueueKind::Steering && !crate::repl::commands::is_slash_command(&msg.text) {
                    ctx.steering.enqueue(msg.text.clone());
                    ctx.controller.set_system_message("[Steering queued for tool boundary]");
                } else if msg.kind == QueueKind::FollowUp {
                    ctx.controller
                        .set_system_message("[Follow-up queued for turn completion]");
                }
            }
            ctx.batch.flush(ctx.controller, true)?;
            Ok(TurnKeyResult::Handled)
        }
        InputAction::HistoryPrevious => {
            if navigate_history_previous(ctx.controller, ctx.history) {
                ctx.batch.flush(ctx.controller, true)?;
            }
            Ok(TurnKeyResult::Handled)
        }
        InputAction::HistoryNext => {
            if navigate_history_next(ctx.controller, ctx.history) {
                ctx.batch.flush(ctx.controller, true)?;
            }
            Ok(TurnKeyResult::Handled)
        }
        InputAction::Complete => {
            if apply_completion(ctx.controller, ctx.completions) {
                ctx.batch.flush(ctx.controller, true)?;
            }
            Ok(TurnKeyResult::Handled)
        }
        InputAction::ToggleExpandTools => {
            let expanded = ctx.controller.toggle_tools_expanded()?;
            ctx.renderer.print_status(&format!(
                "Tool output: {}",
                if expanded { "expanded" } else { "collapsed" }
            ));
            Ok(TurnKeyResult::Handled)
        }
        InputAction::ThinkingToggle => {
            let hide = ctx.controller.toggle_thinking()?;
            ctx.renderer
                .print_status(&format!("Thinking blocks: {}", if hide { "hidden" } else { "visible" }));
            Ok(TurnKeyResult::Handled)
        }
        InputAction::ClipboardPasteImage => {
            paste_clipboard(ctx.renderer, ctx.controller);
            ctx.batch.flush(ctx.controller, true)?;
            Ok(TurnKeyResult::Handled)
        }
        InputAction::DequeueQueued => {
            let queued = ctx.controller.state_mut().dequeue_all();
            if !queued.is_empty() {
                let text = queued.into_iter().map(|m| m.text).collect::<Vec<_>>().join("\n");
                ctx.controller.state_mut().editor_mut().set_text(&text);
                ctx.batch.flush(ctx.controller, true)?;
            }
            Ok(TurnKeyResult::Handled)
        }
        InputAction::Cancel => Ok(TurnKeyResult::Cancelled),
        _ => Ok(TurnKeyResult::Ignored),
    }
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
