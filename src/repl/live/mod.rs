pub mod batch;
pub mod modal;
pub mod navigation;

#[cfg(test)]
mod tests;

pub use batch::LiveController;

use batch::{LiveBatch, OUTPUT_FRAME_INTERVAL, SPINNER_FRAME_INTERVALS, drain_ui_events};
use modal::handle_modal_key;
use navigation::{
    apply_completion, navigate_history_next, navigate_history_previous, restore_queued_messages, update_footer,
};

use crossterm::event::Event;
use tokio::sync::mpsc;

use super::ReplSession;
use super::commands::{CommandResult, SlashCommandContext, SlashCommandHandler};
use super::input_reader::TerminalInputReader;
use super::interactive::{CompletionSet, InteractiveHistory};
use crate::engine::AgentEngine;
use crate::error::Result;
use crate::ui::TerminalRenderer;
use crate::ui::interactive::{
    InputAction, InteractiveState, QueuedMessage, TerminalController, UiEffect, UiEvent, map_key,
};
use crate::ui::render::WelcomeDisplay;

pub struct LiveIo<'a> {
    pub controller: &'a mut LiveController,
    pub events: &'a mut mpsc::UnboundedReceiver<UiEvent>,
    pub input: &'a mut TerminalInputReader,
}

pub struct EditorResources<'a> {
    pub history: &'a mut InteractiveHistory,
    pub completions: &'a CompletionSet,
}

pub struct LiveMessage<'a> {
    pub io: LiveIo<'a>,
    pub editor: EditorResources<'a>,
    pub message: QueuedMessage,
}

pub struct ActiveTurn<'a> {
    pub io: LiveIo<'a>,
    pub editor: EditorResources<'a>,
    pub prompt: &'a str,
}

pub fn live_ui_supported(stdin_is_tty: bool, stdout_is_tty: bool) -> bool {
    stdin_is_tty && stdout_is_tty
}

impl ReplSession {
    pub(super) async fn run_live(&mut self) -> Result<()> {
        let mut engine =
            crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), self.resume_id.as_deref())
                .await?;
        engine.refresh_quota().await;

        let (ui, mut ui_events) = crate::ui::interactive::InteractiveUi::channel();
        self.renderer = TerminalRenderer::with_ui(ui);
        let mut state = InteractiveState::default();
        update_footer(&mut state, self, &engine);
        let mut controller = TerminalController::stdout(state)?;
        let mut input = TerminalInputReader::spawn()?;
        self.renderer.print_welcome(&WelcomeDisplay {
            model: self.config.model.clone(),
            provider: self.config.provider.clone(),
            auto_approve: self.config.auto_approve,
            resumed: self.resume_id.is_some(),
        });
        drain_ui_events(&mut controller, &mut ui_events, &mut None)?;

        let mut history = InteractiveHistory::with_file(1000, self.config.config_dir.join("history.txt"))
            .map_err(|error| anyhow::anyhow!("History unavailable: {error}"))?;
        let skill_names =
            crate::skills::resolved_skills(Some(&self.config.config_dir), std::env::current_dir().ok().as_deref())
                .into_iter()
                .map(|skill| skill.metadata.name)
                .collect();
        let assembly = crate::platform::active_tools(&self.config, &std::env::current_dir()?).await?;
        self.commands = assembly.commands;
        let ext_cmds: Vec<(&str, &str)> = self.commands.keys().map(|k| (k.as_str(), "")).collect();
        let completions = CompletionSet::rho(&ext_cmds, skill_names);

        loop {
            let message = match controller.state_mut().pop_queued() {
                Some(message) => message,
                None => match read_idle_input(
                    LiveIo {
                        controller: &mut controller,
                        events: &mut ui_events,
                        input: &mut input,
                    },
                    &mut history,
                    &completions,
                )
                .await?
                {
                    Some(message) => message,
                    None => break,
                },
            };
            history
                .record(&message.text)
                .map_err(|error| anyhow::anyhow!("History could not be updated: {error}"))?;
            if self
                .process_live_message(
                    &mut engine,
                    LiveMessage {
                        io: LiveIo {
                            controller: &mut controller,
                            events: &mut ui_events,
                            input: &mut input,
                        },
                        editor: EditorResources {
                            history: &mut history,
                            completions: &completions,
                        },
                        message,
                    },
                )
                .await?
            {
                break;
            }
            update_footer(controller.state_mut(), self, &engine);
            controller.redraw()?;
        }
        input.stop_and_join()?;
        Ok(())
    }

    async fn process_live_message(&mut self, engine: &mut AgentEngine, live: LiveMessage<'_>) -> Result<bool> {
        let controller = live.io.controller;
        let ui_events = live.io.events;
        let input_reader = live.io.input;
        let input = live.message.text.trim();
        let command_result = if input.starts_with('/') {
            let paused_input = input_reader.pause()?;
            controller.suspend()?;
            let mut command_context = SlashCommandContext {
                config: &mut self.config,
                auth_store: &mut self.auth_store,
                renderer: &self.renderer,
                commands: Some(&self.commands),
                session_id: Some(&engine.session_manager.session_id),
            };
            let result = SlashCommandHandler::handle(input, &mut command_context).await;
            let controller_result = controller.resume();
            let input_result = paused_input.resume();
            controller_result?;
            input_result?;
            result?
        } else {
            None
        };
        if let Some(result) = command_result {
            match result {
                CommandResult::Exit => return Ok(true),
                CommandResult::ClearContext => {
                    *engine = crate::platform::agent_engine(self.config.clone(), self.auth_store.clone(), None).await?;
                }
                CommandResult::ModelChanged {
                    new_model,
                    new_provider,
                } => {
                    self.config.model = new_model;
                    if let Some(provider) = new_provider {
                        self.config.provider = provider;
                    }
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                }
                CommandResult::Login { provider } => {
                    crate::cli::login_provider(provider.as_deref(), &self.config, &mut self.auth_store).await?;
                    *engine = engine.rebuild(self.config.clone(), self.auth_store.clone()).await?;
                }
                CommandResult::Compact { .. } => {
                    let session_id = engine.session_manager.session_id.clone();
                    self.renderer.print_notice("  [Compacting conversation context...]\n");
                    let memory = crate::session::context::context_memory(
                        engine.session_manager.clone(),
                        1,
                        self.config.compaction_max_bytes,
                    );
                    let _ = memory.load(&session_id).await;
                    self.renderer.print_notice("  [Context compaction completed]\n");
                }
                CommandResult::Tree => {
                    let turns = engine.session_manager.load_turns().await?;
                    let mut out = format!("\nConversation Tree (Session: {})\n", engine.session_manager.session_id);
                    if turns.is_empty() {
                        out.push_str("  (No conversation turns recorded yet)\n");
                    } else {
                        let total = turns.len();
                        for (idx, turn) in turns.iter().enumerate() {
                            let is_last = idx + 1 == total;
                            let marker = if is_last { "└──" } else { "├──" };
                            let current_tag = if is_last { " (Current)" } else { "" };
                            let prompt_preview = if turn.user_prompt.chars().count() > 40 {
                                format!("{}...", turn.user_prompt.chars().take(37).collect::<String>())
                            } else {
                                turn.user_prompt.clone()
                            };
                            let assistant_preview = if turn.assistant_preview.chars().count() > 40 {
                                format!("{}...", turn.assistant_preview.chars().take(37).collect::<String>())
                            } else {
                                turn.assistant_preview.clone()
                            };
                            let tools_tag = if turn.tool_calls_count > 0 {
                                format!(" ({} tool calls)", turn.tool_calls_count)
                            } else {
                                String::new()
                            };
                            use std::fmt::Write as _;
                            let _ = writeln!(
                                out,
                                "  {marker} [Turn {}]{current_tag} User: \"{}\" -> Assistant: \"{}\"{tools_tag}",
                                turn.turn_number, prompt_preview, assistant_preview
                            );
                        }
                        out.push_str("  (Use /rewind <turn_number> to fork or rewind context)\n");
                    }
                    self.renderer.print_notice(&out);
                }
                CommandResult::Rewind { turn } => {
                    let retained_count = engine.session_manager.rewind_to_turn(turn).await?;
                    self.renderer.print_notice(&format!(
                        "  [Rewound context to Turn {turn} ({retained_count} messages retained)]\n"
                    ));
                }
                CommandResult::Logout { provider } => {
                    crate::cli::logout_provider(provider.as_deref(), &self.config, &mut self.auth_store)?;
                }
                CommandResult::Continue => {}
            }
            drain_ui_events(controller, ui_events, &mut None)?;
            return Ok(false);
        }

        let effective = input.to_string();
        self.renderer.print_user_block(&effective);
        run_active_turn(
            engine,
            &self.renderer,
            ActiveTurn {
                io: LiveIo {
                    controller,
                    events: ui_events,
                    input: input_reader,
                },
                editor: live.editor,
                prompt: &effective,
            },
        )
        .await?;
        engine.refresh_quota().await;
        Ok(false)
    }
}

async fn read_idle_input(
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

async fn run_active_turn(engine: &AgentEngine, renderer: &TerminalRenderer, turn: ActiveTurn<'_>) -> Result<()> {
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
                spinner_tick += 1;
                if spinner_tick >= SPINNER_FRAME_INTERVALS {
                    spinner_tick = 0;
                    controller.advance_spinner();
                }
                batch.flush(controller, false)?;
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
                let Event::Key(key) = event else { continue };
                if handle_modal_key(controller, key, &mut batch.modal)? {
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
                        controller.toggle_tools_expanded()?;
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
