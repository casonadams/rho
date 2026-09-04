mod editor;
mod modal_action;
mod shortcut;

use editor::open_external_editor;
use modal_action::{ModalActionContext, apply_modal_key_result};
use shortcut::{IdleShortcutContext, handle_shortcut_action};

use super::IdleContext;
use super::autocomplete::{AutocompleteKeyResult, handle_autocomplete_key, update_autocomplete_state};
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use super::modal::handle_modal_key;
use super::navigation::{apply_completion, navigate_history_next, navigate_history_previous};
use crate::error::Result;
use crate::ui::interactive::{InputAction, QueuedMessage, UiAction, UiEffect, map_key};
use crossterm::event::Event;

pub(crate) async fn read_idle_input<B: crate::ui::interactive::TerminalBackend>(
    ctx: IdleContext<'_, '_, B>,
) -> Result<Option<QueuedMessage>> {
    let controller = ctx.io.controller;
    let ui_events = ctx.io.events;
    let input = ctx.io.input;
    let history = ctx.editor.history;
    let completions = ctx.editor.completions;
    let session = &mut *ctx.session;
    let engine = &mut *ctx.engine;
    let mut batch = LiveBatch::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_escape_time: Option<std::time::Instant> = None;

    loop {
        tokio::select! {
            biased;
            _ = frame.tick() => batch.flush(controller, false)?,
            event = input.recv() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        batch.flush(controller, false)?;
                        return Err(error.into());
                    }
                    None => {
                        batch.flush(controller, false)?;
                        return Err(anyhow::anyhow!("Terminal input reader stopped").into());
                    }
                };
                if matches!(event, Event::Resize(_, _)) {
                    controller.refresh_size()?;
                    continue;
                }
                if let Event::Paste(text) = event {
                    controller.state_mut().apply(UiAction::Paste(text));
                    update_autocomplete_state(controller, completions);
                    batch.flush(controller, true)?;
                    continue;
                }
                let Event::Key(key) = event else { continue };
                let modal_res = handle_modal_key(controller, key, &mut batch.modal)?;
                if apply_modal_key_result(
                    modal_res,
                    ModalActionContext {
                        controller,
                        history,
                        session,
                        engine,
                    },
                    &mut batch,
                ).await? {
                    continue;
                }
                if matches!(handle_autocomplete_key(controller, completions, key), AutocompleteKeyResult::Handled) {
                    batch.flush(controller, true)?;
                    continue;
                }
                match map_key(key) {
                    InputAction::Edit(action) => {
                        let effect = controller.state_mut().apply(action);
                        update_autocomplete_state(controller, completions);
                        if let UiEffect::Queued(message) = effect {
                            controller.state_mut().pop_queued();
                            batch.flush(controller, true)?;
                            return Ok(Some(message));
                        }
                        batch.flush(controller, true)?;
                    }
                    InputAction::HistoryPrevious => {
                        if navigate_history_previous(controller, history) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::HistoryNext => {
                        if navigate_history_next(controller, history) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::Complete => {
                        if apply_completion(controller, completions) {
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::ExternalEditor => {
                        open_external_editor(controller, input)?;
                        batch.flush(controller, true)?;
                    }
                    InputAction::DequeueQueued => {
                        let queued = controller.state_mut().dequeue_all();
                        if !queued.is_empty() {
                            let text = queued
                                .into_iter()
                                .map(|m| m.text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            controller.state_mut().editor_mut().set_text(&text);
                            batch.flush(controller, true)?;
                        }
                    }
                    InputAction::EndOfInput if controller.state().editor().is_empty() => {
                        batch.flush(controller, false)?;
                        return Ok(None);
                    }
                    InputAction::EndOfInput | InputAction::Ignore => {}
                    action => {
                        handle_shortcut_action(
                            action,
                            IdleShortcutContext {
                                controller,
                                session,
                                engine,
                                last_escape_time: &mut last_escape_time,
                            },
                            &mut batch,
                        ).await?;
                    }
                }
            }
            event = ui_events.recv() => {
                if let Some(event) = event {
                    batch.enqueue(controller, event)?;
                }
            }
        }
    }
}
