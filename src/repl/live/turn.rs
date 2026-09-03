use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVALS};
use super::modal::handle_modal_key;
use super::navigation::{
    apply_completion, navigate_history_next, navigate_history_previous, paste_clipboard, restore_queued_messages,
};
use super::{ActiveTurn, EditorResources, LiveIo};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{Activity, InputAction, UiAction, map_key};
use crossterm::event::Event;

pub(crate) async fn run_active_turn<B: crate::ui::interactive::TerminalBackend>(
    engine: &AgentEngine,
    renderer: &TerminalRenderer,
    turn: ActiveTurn<'_, B>,
) -> Result<()> {
    let ActiveTurn {
        io: LiveIo {
            controller,
            events: ui_events,
            input: input_reader,
        },
        editor: EditorResources { history, completions },
        prompt,
    } = turn;

    let request = crate::engine::runner::TurnRequest::new(prompt);
    let mut batch = LiveBatch::new();
    let mut frame = tokio::time::interval(OUTPUT_FRAME_INTERVAL);
    frame.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut spinner_tick = 0_usize;
    let mut run = std::pin::pin!(engine.run_turn(request, std::sync::Arc::new(renderer.clone())));

    loop {
        tokio::select! {
            biased;
            _ = frame.tick() => {
                if controller.state().active_modal().is_some() {
                    batch.flush(controller, false)?;
                    continue;
                }
                spinner_tick += 1;
                let spinner_advanced = if spinner_tick >= SPINNER_FRAME_INTERVALS {
                    spinner_tick = 0;
                    controller.advance_spinner();
                    !matches!(controller.state().footer().activity, Activity::Idle)
                } else {
                    false
                };
                batch.flush(controller, spinner_advanced)?;
            }
            event = input_reader.recv() => {
                let event = match event {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        batch.flush(controller, false)?;
                        return Err(error.into());
                    }
                    None => continue,
                };
                if matches!(event, Event::Resize(_, _)) {
                    controller.refresh_size()?;
                    continue;
                }
                if let Event::Paste(text) = event {
                    controller.state_mut().apply(UiAction::Paste(text));
                    batch.flush(controller, true)?;
                    continue;
                }
                let Event::Key(key) = event else { continue };
                if !matches!(
                    handle_modal_key(controller, key, &mut batch.modal)?,
                    super::modal::ModalKeyResult::NotHandled
                ) {
                    continue;
                }
                match map_key(key) {
                    InputAction::Edit(action) => {
                        controller.state_mut().apply(action);
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
                    InputAction::ToggleExpandTools => {
                        let expanded = controller.toggle_tools_expanded()?;
                        renderer.print_status(&format!(
                            "Tool output: {}",
                            if expanded { "expanded" } else { "collapsed" }
                        ));
                    }
                    InputAction::ThinkingToggle => {
                        let hide = controller.toggle_thinking()?;
                        renderer.print_status(&format!(
                            "Thinking blocks: {}",
                            if hide { "hidden" } else { "visible" }
                        ));
                    }
                    InputAction::ClipboardPasteImage => {
                        paste_clipboard(renderer, controller);
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
                    InputAction::Cancel => {
                        batch.flush(controller, false)?;
                        engine.record_cancellation("operator interrupt").await?;
                        restore_queued_messages(controller);
                        renderer.print_notice("\nCanceled.\n");
                        batch.drain_events(controller, ui_events)?;
                        batch.flush(controller, false)?;
                        return Ok(());
                    }
                    _ => {}
                }
            }
            result = &mut run => {
                renderer.flush();
                batch.drain_events(controller, ui_events)?;
                batch.flush(controller, false)?;
                if let Err(error) = result {
                    restore_queued_messages(controller);
                    renderer.print_notice(&format!("\nError: {error}\n"));
                    batch.drain_events(controller, ui_events)?;
                    batch.flush(controller, false)?;
                }
                return Ok(());
            }
            event = ui_events.recv() => {
                if let Some(event) = event {
                    batch.enqueue(controller, event)?;
                }
            }
        }
    }
}
