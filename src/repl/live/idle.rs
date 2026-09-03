use super::IdleContext;
use super::autocomplete::{AutocompleteKeyResult, handle_autocomplete_key, update_autocomplete_state};
use super::batch::{LiveBatch, OUTPUT_FRAME_INTERVAL};
use super::modal::{ModalKeyResult, handle_modal_key, open_model_selector};
use super::navigation::{
    ModelCycleContext, apply_completion, copy_last_message, cycle_model, cycle_thinking_level, navigate_history_next,
    navigate_history_previous, paste_clipboard, update_footer,
};
use crate::error::Result;
use crate::ui::interactive::{InputAction, QueuedMessage, UiAction, UiEffect, map_key};
use crossterm::event::Event;

pub(crate) async fn read_idle_input(ctx: IdleContext<'_, '_>) -> Result<Option<QueuedMessage>> {
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
                match handle_modal_key(controller, key, &mut batch.modal)? {
                    ModalKeyResult::Handled => continue,
                    ModalKeyResult::ModelSelected {
                        model,
                        provider,
                        save_as_default,
                    } => {
                        session.config.model = model.clone();
                        session.config.provider = provider.clone();
                        let _ = rho_harness_core::state::AppState::set_last_model(
                            &session.config.config_dir,
                            &model,
                            Some(&provider),
                        );
                        if save_as_default {
                            let _ = rho_harness_core::config::Config::set_file_value(
                                &session.config.config_dir,
                                "model",
                                &model,
                            );
                            let _ = rho_harness_core::config::Config::set_file_value(
                                &session.config.config_dir,
                                "provider",
                                &provider,
                            );
                            session
                                .renderer
                                .print_status(&format!("Default model: {model} ({provider})"));
                        } else {
                            session.renderer.print_status(&format!("Model: {model} ({provider})"));
                        }
                        if let Ok(rebuilt) = engine.rebuild(session.config.clone(), session.auth_store.clone()).await {
                            *engine = rebuilt;
                        }
                        update_footer(controller.state_mut(), session, engine);
                        batch.flush(controller, true)?;
                        continue;
                    }
                    ModalKeyResult::TreeNodeSelected { node_id } => {
                        match engine.session_manager.switch_branch(Some(node_id.clone())).await {
                            Ok(_) => {
                                if let Ok(tree) = engine.session_manager.load_tree().await {
                                    let _ = super::navigation::hydrate_session_transcript(controller, &tree, history);
                                }
                                session.renderer.print_status(&format!("Navigated to checkpoint {node_id}"));
                            }
                            Err(err) => {
                                session.renderer.print_status(&format!("Failed to navigate: {err}"));
                            }
                        }
                        controller.redraw()?;
                        continue;
                    }
                    ModalKeyResult::NodeLabelUpdated { node_id, label } => {
                        let label_opt = if label.is_empty() { None } else { Some(label.clone()) };
                        match engine.session_manager.set_node_label(&node_id, label_opt).await {
                            Ok(_) => {
                                session
                                    .renderer
                                    .print_status(&format!("Checkpoint labeled: \"{label}\" ({node_id})"));
                            }
                            Err(err) => {
                                session.renderer.print_status(&format!("Failed to label checkpoint: {err}"));
                            }
                        }
                        controller.redraw()?;
                        continue;
                    }
                    ModalKeyResult::SessionSelected { session_id } => {
                        *engine = crate::platform::agent_engine(
                            session.config.clone(),
                            session.auth_store.clone(),
                            Some(&session_id),
                        )
                        .await?;
                        if let Ok(tree) = engine.session_manager.load_tree().await {
                            let _ = super::navigation::hydrate_session_transcript(controller, &tree, history);
                        }
                        session.renderer.print_status(&format!("Resumed session {session_id}"));
                        update_footer(controller.state_mut(), session, engine);
                        controller.redraw()?;
                        continue;
                    }
                    ModalKeyResult::NotHandled => {}
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
                    InputAction::Cancel => {
                        let was_empty = controller.state().editor().text().is_empty();
                        controller.state_mut().autocomplete.close();
                        controller.state_mut().editor_mut().set_text("");
                        if was_empty {
                            let now = std::time::Instant::now();
                            if let Some(prev) = last_escape_time.take() {
                                if now.duration_since(prev) < std::time::Duration::from_millis(500) {
                                    if let Ok(tree) = engine.session_manager.load_tree().await {
                                        super::modal::open_tree_selector(&tree, controller);
                                    }
                                } else {
                                    last_escape_time = Some(now);
                                }
                            } else {
                                last_escape_time = Some(now);
                            }
                        } else {
                            last_escape_time = None;
                        }
                        controller.redraw()?;
                    }
                    InputAction::ToggleExpandTools => {
                        let expanded = controller.toggle_tools_expanded()?;
                        session.renderer.print_status(&format!(
                            "Tool output: {}",
                            if expanded { "expanded" } else { "collapsed" }
                        ));
                    }
                    InputAction::ModelSelect => {
                        open_model_selector(session, controller);
                        controller.redraw()?;
                    }
                    InputAction::ModelCycleForward => {
                        let mut cycle_ctx = ModelCycleContext { session, engine, controller };
                        cycle_model(&mut cycle_ctx, 1).await;
                        batch.flush(controller, true)?;
                    }
                    InputAction::ModelCycleBackward => {
                        let mut cycle_ctx = ModelCycleContext { session, engine, controller };
                        cycle_model(&mut cycle_ctx, -1).await;
                        batch.flush(controller, true)?;
                    }
                    InputAction::ThinkingCycle => {
                        cycle_thinking_level(session, engine, controller);
                        batch.flush(controller, true)?;
                    }
                    InputAction::ThinkingToggle => {
                        let hide = controller.toggle_thinking()?;
                        session.renderer.print_status(&format!(
                            "Thinking blocks: {}",
                            if hide { "hidden" } else { "visible" }
                        ));
                    }
                    InputAction::MessageCopy => {
                        copy_last_message(session, controller);
                        batch.flush(controller, true)?;
                    }
                    InputAction::ClipboardPasteImage => {
                        paste_clipboard(&session.renderer, controller);
                        batch.flush(controller, true)?;
                    }
                    InputAction::SessionTree => {
                        if let Ok(tree) = engine.session_manager.load_tree().await {
                            super::modal::open_tree_selector(&tree, controller);
                            controller.redraw()?;
                        }
                    }
                    InputAction::SessionResume => {
                        super::modal::open_session_selector(&session.config.sessions_dir, controller);
                        controller.redraw()?;
                    }
                    InputAction::SessionNew => {
                        *engine = crate::platform::agent_engine(
                            session.config.clone(),
                            session.auth_store.clone(),
                            None,
                        )
                        .await?;
                        controller.clear_transcript();
                        session.renderer.print_status("Context cleared");
                        update_footer(controller.state_mut(), session, engine);
                        controller.redraw()?;
                    }
                    InputAction::Suspend => {
                        crate::platform::suspend::suspend_process();
                        controller.redraw()?;
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
