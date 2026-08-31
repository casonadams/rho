use super::LiveIo;
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use super::modal::handle_modal_key;
use super::navigation::{apply_completion, navigate_history_next, navigate_history_previous};
use crate::error::Result;
use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{InputAction, QueuedMessage, UiEffect, map_key};
use crossterm::event::Event;

pub(crate) async fn read_idle_input(
    live: LiveIo<'_>,
    history: &mut InteractiveHistory,
    completions: &CompletionSet,
) -> Result<Option<QueuedMessage>> {
    let controller = live.controller;
    let ui_events = live.events;
    let input = live.input;
    let mut batch = LiveBatch::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
                let Event::Key(key) = event else { continue };
                if handle_modal_key(controller, key, &mut batch.modal)? {
                    continue;
                }
                match map_key(key) {
                    InputAction::Edit(action) => {
                        let effect = controller.state_mut().apply(action);
                        batch.flush(controller, true)?;
                        if let UiEffect::Queued(message) = effect {
                            controller.state_mut().pop_queued();
                            return Ok(Some(message));
                        }
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
                    InputAction::Cancel => {
                        controller.state_mut().editor_mut().set_text("");
                        controller.redraw()?;
                    }
                    InputAction::ToggleExpandTools => {
                        controller.toggle_tools_expanded()?;
                    }
                    InputAction::ExternalEditor => {
                        let current_text = controller.state().editor().text().to_string();
                        let temp_file =
                            std::env::temp_dir().join(format!("rho_draft_{}.md", uuid::Uuid::new_v4()));
                        let _ = std::fs::write(&temp_file, &current_text);
                        let editor = std::env::var("VISUAL")
                            .or_else(|_| std::env::var("EDITOR"))
                            .unwrap_or_else(|_| "nano".to_string());
                        let paused = input.pause()?;
                        controller.suspend()?;
                        let status = std::process::Command::new(&editor)
                            .arg(&temp_file)
                            .status();
                        let controller_res = controller.resume();
                        let input_res = paused.resume();
                        controller_res?;
                        input_res?;
                        if status.is_ok()
                            && let Ok(edited_text) = std::fs::read_to_string(&temp_file)
                        {
                            controller
                                .state_mut()
                                .editor_mut()
                                .set_text(edited_text.trim_end());
                        }
                        let _ = std::fs::remove_file(temp_file);
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
